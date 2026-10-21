import importlib.util
import json
import sqlite3
import tempfile
import subprocess
import sys
import unittest
from unittest.mock import patch
from pathlib import Path

spec = importlib.util.spec_from_file_location('sonefold', Path(__file__).parents[1] / 'sonefold.py')
sonefold = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sonefold)


class IntegrationsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.db = self.directory / 'meetings.sqlite'
        self.config = self.directory / 'integrations.json'
        with sqlite3.connect(self.db) as db:
            db.executescript('''
                CREATE TABLE meetings(id TEXT PRIMARY KEY,title TEXT,created_at TEXT,updated_at TEXT);
                CREATE TABLE transcripts(id TEXT,meeting_id TEXT,transcript TEXT,timestamp TEXT,audio_start_time REAL,audio_end_time REAL);
                CREATE TABLE summary_processes(meeting_id TEXT,result TEXT);
                INSERT INTO meetings VALUES('meeting-1','Multilingual meeting','2026-09-30','2026-09-30');
                INSERT INTO summary_processes VALUES('meeting-1','{"markdown":"A decision"}');
            ''')
            db.executemany('INSERT INTO transcripts VALUES(?,?,?,?,?,?)', [
                (f't-{i}', 'meeting-1', '日本語 100% correct' if i == 0 else f'part {i}', '12:00', i, i + 1) for i in range(503)])
        self.grant = dict(database_path=str(self.db), cli_enabled=True, mcp_enabled=True,
                          read_meetings=True, read_transcripts=True, read_summaries=True)
        self.save()
        self.reader = sonefold.MeetingReader(self.config, 'cli')

    def save(self):
        self.config.write_text(json.dumps(self.grant))

    def test_protected_reads_use_audited_gateway_and_recheck_policy(self):
        with sqlite3.connect(self.db) as db:
            db.execute('CREATE TABLE privacy_settings(id INTEGER, policy TEXT)')
            db.execute('INSERT INTO privacy_settings VALUES(1, ?)', (json.dumps({'protected_mode': True}),))
        with patch.object(self.reader, '_app_call', return_value={'audited': True}) as call:
            self.assertEqual(self.reader.meetings(), {'audited': True})
            self.assertEqual(call.call_args.args[0], 'list_meetings')
            self.reader.search('part')
            self.assertEqual(call.call_args.args[0], 'search_transcripts')
            self.reader.meeting('meeting-1', include_transcripts=False)
            self.assertEqual(call.call_args.args[0], 'get_meeting_details')
            self.assertFalse(call.call_args.args[1]['include_transcripts'])
            with self.assertRaises(sonefold.AuditedReadRequired): self.reader.connect()
            self.grant['read_summaries'] = False
            self.save()
            call.reset_mock()
            with self.assertRaises(PermissionError): self.reader.meeting('meeting-1')
            call.assert_not_called()
        with sqlite3.connect(self.db) as db:
            db.execute('UPDATE privacy_settings SET policy=?', (json.dumps({'protected_mode': False}),))
        self.assertEqual(len(self.reader.meetings()), 1)

    def test_invalid_privacy_policy_cannot_fall_back_to_unaudited_read(self):
        with sqlite3.connect(self.db) as db:
            db.execute('CREATE TABLE privacy_settings(id INTEGER, policy TEXT)')
            db.execute('INSERT INTO privacy_settings VALUES(1, ?)', ('broken-json',))
        with patch.object(self.reader, '_app_call') as call:
            with self.assertRaises(ValueError): self.reader.meetings()
            call.assert_not_called()

    def test_disabled_by_default_and_revocation_on_next_request(self):
        self.assertEqual(len(self.reader.meetings()), 1)
        self.grant['cli_enabled'] = False
        self.save()
        with self.assertRaises(PermissionError): self.reader.meetings()
        self.grant.pop('cli_enabled')
        self.save()
        with self.assertRaises(PermissionError): self.reader.meetings()

    def test_grants_do_not_expose_unapproved_content(self):
        self.grant['read_transcripts'] = False
        self.save()
        with self.assertRaises(PermissionError): self.reader.search('part')
        with self.assertRaises(PermissionError): self.reader.meeting('meeting-1')
        summary = self.reader.meeting('meeting-1', include_transcripts=False)
        self.assertNotIn('transcripts', summary)
        self.assertEqual(summary['summary']['markdown'], 'A decision')

    def test_metadata_revocation_also_blocks_transcript_search(self):
        self.grant['read_meetings'] = False
        self.save()
        with self.assertRaises(PermissionError): self.reader.search('part')
        with self.assertRaises(PermissionError): self.reader.meeting('meeting-1')

    def test_grants_and_content_flags_require_booleans(self):
        self.grant['cli_enabled'] = 'false'
        self.save()
        with self.assertRaises(PermissionError): self.reader.meetings()
        self.grant['cli_enabled'] = True
        self.save()
        with self.assertRaises(ValueError): self.reader.meeting('meeting-1', include_transcripts='false')

    def test_complete_export_contains_last_page_and_unicode(self):
        text = self.reader.markdown('meeting-1')
        self.assertIn('日本語 100% correct', text)
        self.assertIn('part 502', text)
        self.assertIn('A decision', text)
        self.assertEqual(text.count('part '), 502)

    def test_search_literal_wildcard_and_sql_injection(self):
        self.assertEqual(len(self.reader.search('100%')), 1)
        self.assertEqual(self.reader.search("' OR 1=1 --"), [])
        with self.reader.connect() as db:
            with self.assertRaises(sqlite3.OperationalError): db.execute('DELETE FROM meetings')

    def test_pagination_range_validation(self):
        for value in (0, 501, -1, True, 1.5, '10'):
            with self.assertRaises(ValueError): self.reader.meetings(value)
        with self.assertRaises(ValueError): self.reader.meetings(10, -1)
        page = self.reader.meeting('meeting-1', 100, 500)
        self.assertEqual(len(page['transcripts']), 3)
        self.assertFalse(page['has_more'])

    def test_mcp_initialization_and_read_only_tools(self):
        reply = sonefold.rpc(self.reader, dict(jsonrpc='2.0', id=1, method='initialize', params={'protocolVersion': '2025-06-18'}))
        self.assertEqual(reply['result']['protocolVersion'], '2025-06-18')
        result = sonefold.rpc(self.reader, dict(jsonrpc='2.0', id=2, method='tools/list'))
        self.assertTrue(all(tool['annotations']['readOnlyHint'] for tool in result['result']['tools']))
        self.assertIsNone(sonefold.rpc(self.reader, dict(jsonrpc='2.0', method='notifications/initialized')))
        invalid = sonefold.rpc(self.reader, [])
        self.assertEqual(invalid['error']['code'], -32600)

    def test_mcp_errors_remain_json_rpc_and_permission_failures_tool_errors(self):
        self.grant['read_transcripts'] = False
        self.save()
        response = sonefold.rpc(self.reader, dict(jsonrpc='2.0', id=7, method='tools/call', params={'name': 'search_transcripts', 'arguments': {'query': 'part'}}))
        self.assertTrue(response['result']['isError'])
        bad = sonefold.rpc(self.reader, dict(jsonrpc='2.0', id=8, method='tools/call', params={'name': 'get_meeting', 'arguments': {'path': '/etc/passwd'}}))
        self.assertEqual(bad['error']['code'], -32602)

    def test_mcp_malformed_messages_do_not_terminate_server(self):
        malformed = [
            b'[' * 20000 + b'0' + b']' * 20000,
            b'{"jsonrpc":"2.0","id":' + b'9' * 10000 + b',"method":"ping"}',
            b'{"jsonrpc":"2.0","id":NaN,"method":"ping"}',
            b'{"jsonrpc":"2.0","id":Infinity,"method":"ping"}',
            b'{"jsonrpc":"2.0","id":1e999,"method":"ping"}',
            b'\xff',
        ]
        ping = b'{"jsonrpc":"2.0","id":42,"method":"ping"}'
        result = subprocess.run(
            [sys.executable, str(Path(sonefold.__file__)), '--config', str(self.config), 'mcp'],
            input=b'\n'.join([*malformed, ping]) + b'\n', capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies), len(malformed) + 1)
        self.assertTrue(all(reply['error']['code'] == -32700 for reply in replies[:-1]))
        self.assertEqual(replies[-1], {'jsonrpc': '2.0', 'id': 42, 'result': {}})

    def test_mcp_rejects_invalid_ids_and_falsy_nonobject_params(self):
        for value in (None, True, 1.5, [], {}):
            with self.subTest(id=value):
                reply = sonefold.rpc(self.reader, dict(jsonrpc='2.0', id=value, method='ping'))
                self.assertEqual(reply['error']['code'], -32600)
                self.assertIsNone(reply['id'])
        for value in ([], False, '', 0):
            with self.subTest(params=value):
                reply = sonefold.rpc(self.reader, dict(jsonrpc='2.0', id=1, method='ping', params=value))
                self.assertEqual(reply['error']['code'], -32602)

    def test_reads_close_connections_including_failed_lookup(self):
        for operation in (lambda: self.reader.meetings(), lambda: self.reader.search('part'),
                          lambda: self.reader.meeting('meeting-1'),
                          lambda: self.reader.meeting('missing')):
            connection = self.reader.connect()
            with patch.object(self.reader, 'connect', return_value=connection):
                try:
                    operation()
                except LookupError:
                    pass
            with self.assertRaises(sqlite3.ProgrammingError):
                connection.execute('SELECT 1')

    def proof(self, token, nonce='0' * 32, port=8421):
        mac = sonefold.hmac.new(token.encode(), f'overtalk-local-api-proof\n{port}\n{nonce}'.encode(), sonefold.hashlib.sha256)
        return json.dumps({'proof': mac.hexdigest()}).encode()

    def test_native_control_requires_grant_uses_local_client_token_and_revokes(self):
        with patch.object(sonefold.http.client, 'HTTPConnection') as factory, \
                patch.object(sonefold.secrets, 'token_hex', return_value='0' * 32):
            with self.assertRaises(PermissionError): self.reader.control('start_recording')
            factory.assert_not_called()
            self.grant.update(record_control=True, cli_token='c' * 64, api_port=8421)
            self.save()
            response = factory.return_value.getresponse.return_value
            response.status = 200
            response.read.side_effect = [self.proof('c' * 64), b'{"result":{"started":true}}']
            self.assertTrue(self.reader.control('start_recording', title='Japanese 日本語')['started'])
            self.assertEqual(factory.call_args_list[0].args, ('127.0.0.1', 8421))
            factory.assert_called_with('127.0.0.1', 8421, timeout=600)
            check, request = factory.return_value.request.call_args_list
            self.assertEqual(check.args, ('GET', '/v1/proof'))
            self.assertNotIn('Authorization', check.kwargs['headers'])
            self.assertEqual(request.kwargs['headers']['Authorization'], 'Bearer ' + 'c' * 64)
            body = json.loads(request.kwargs['body'])
            self.assertEqual(body['arguments']['title'], 'Japanese 日本語')
            self.assertEqual(factory.return_value.close.call_count, 2)
            self.grant['record_control'] = False
            self.save()
            with self.assertRaises(PermissionError): self.reader.control('start_recording')
            self.assertEqual(factory.call_count, 2)

    def test_a_listener_that_cannot_prove_the_token_never_receives_it(self):
        self.grant.update(record_control=True, cli_token='c' * 64, api_port=8421)
        self.save()
        with patch.object(sonefold.http.client, 'HTTPConnection') as factory:
            response = factory.return_value.getresponse.return_value
            response.status = 200
            response.read.return_value = json.dumps({'proof': '0' * 64}).encode()
            with self.assertRaises(PermissionError): self.reader.control('start_recording')
            self.assertEqual(factory.call_count, 1)
            self.assertNotIn('Authorization', factory.return_value.request.call_args.kwargs['headers'])

    def test_mcp_control_catalog_is_scoped_and_calls_cannot_bypass_it(self):
        reader = sonefold.MeetingReader(self.config, 'mcp')
        names = {tool['name'] for tool in sonefold.available_tools(reader)}
        self.assertNotIn('start_recording', names)
        denied = sonefold.rpc(reader, dict(jsonrpc='2.0', id=10, method='tools/call', params={'name': 'start_recording', 'arguments': {}}))
        self.assertTrue(denied['result']['isError'])
        self.grant['record_control'] = True
        self.save()
        tools = sonefold.available_tools(reader)
        self.assertFalse(next(tool for tool in tools if tool['name'] == 'start_recording')['annotations']['readOnlyHint'])
        self.assertNotIn('regenerate_summary', {tool['name'] for tool in tools})
        self.grant['write_notes'] = True
        self.save()
        self.assertIn('regenerate_summary', {tool['name'] for tool in sonefold.available_tools(reader)})

    def test_malformed_control_arguments_do_not_contact_gateway(self):
        self.grant.update(write_notes=True, cli_token='c' * 64)
        self.save()
        with patch.object(sonefold.http.client, 'HTTPConnection') as factory:
            for args in ({}, {'meeting_id': 'one', 'path': '/etc/passwd'}, {'meeting_id': True}):
                with self.assertRaises(ValueError): self.reader.control('regenerate_summary', **args)
            factory.assert_not_called()

    def test_editor_summary_exports_current_document_without_stale_markdown(self):
        summary = {'markdown': 'Stale decision', 'summary_json': [
            {'type': 'heading', 'props': {'level': 2}, 'content': [{'text': '決定'}]},
            {'type': 'paragraph', 'content': [{'text': 'Updated decision 日本語'}]},
            {'type': 'table', 'content': {'rows': [{'cells': [[{'text': 'Owner'}], [{'text': 'Date'}]]},
                                                     {'cells': [[{'text': 'Alice'}], [{'text': 'Friday'}]]}]}}
        ]}
        text = sonefold.summary_markdown(summary)
        self.assertIn('## 決定', text)
        self.assertIn('Updated decision 日本語', text)
        self.assertIn('| Alice | Friday |', text)
        self.assertNotIn('Stale decision', text)
        self.assertEqual(sonefold.summary_markdown({'markdown': 'Stale decision', 'summary_json': []}), '')

    def test_hybrid_library_grants_revoke_catalog_and_dispatch(self):
        reader = sonefold.MeetingReader(self.config, 'mcp')
        self.assertIn('search_library', {t['name'] for t in sonefold.available_tools(reader)})
        for grant in ('read_meetings', 'read_transcripts', 'read_summaries'):
            self.grant[grant] = False
            self.save()
            self.assertNotIn('search_library', {t['name'] for t in sonefold.available_tools(reader)})
            with patch.object(reader, '_app_call') as native:
                with self.assertRaises(PermissionError): reader.library('decision')
                native.assert_not_called()
            self.grant[grant] = True
        self.save()

    def test_hybrid_library_filters_reach_native_only_after_validation(self):
        with patch.object(self.reader, '_app_call', return_value={'sources': [], 'mode': 'full-text'}) as native:
            result = self.reader.library('日本語 launch', semantic=False, tag='product', after='2026-09-01')
            self.assertEqual(result['mode'], 'full-text')
            self.assertEqual(native.call_args.args[0], 'search_library')
            self.assertEqual(native.call_args.args[1]['tag'], 'product')
            self.assertFalse(native.call_args.args[1]['semantic'])
            native.reset_mock()
            for invalid in ({'semantic': 'false'}, {'limit': True}, {'limit': 101}, {'tag': True}, {'query': ' '}):
                arguments = {'query': 'launch', **invalid}
                with self.assertRaises(ValueError): self.reader.library(**arguments)
            native.assert_not_called()

    def test_legacy_summary_export_preserves_declared_order(self):
        summary = {'later': {'title': 'Later', 'blocks': [{'type': 'paragraph', 'content': 'Second'}]},
                   'first': {'title': 'First', 'blocks': [{'type': 'bullet', 'content': 'First item'}]},
                   '_section_order': ['first', 'later']}
        text = sonefold.summary_markdown(summary)
        self.assertLess(text.index('## First'), text.index('## Later'))
        self.assertIn('- First item', text)


if __name__ == '__main__': unittest.main()
