"""Real service privacy checks in a disposable synthetic library.

No physical microphone, real meeting, provider account or display change.
"""
from service_client import Service
import argparse
import json
import os
from pathlib import Path
import sqlite3
import tempfile



def rejected(call, text):
    try:
        call()
    except RuntimeError as error:
        assert text.casefold() in str(error).casefold(), error
    else:
        raise AssertionError('Sensitive operation unexpectedly succeeded')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    checks = {}
    with tempfile.TemporaryDirectory(prefix='overtalk-privacy-check-') as temporary:
        root = Path(temporary)
        service = Service(args.binary.resolve(), root)
        try:
            privacy = lambda **action: service.call('workspace_privacy', action=action)
            status = privacy(op='status')
            assert status['policy']['require_recording_notice']
            pref = service.call('get_recording_preferences')
            recordings = root / 'recordings'
            recordings.mkdir()
            pref['save_folder'] = str(recordings)
            service.call('set_recording_preferences', preferences=pref)
            folder = recordings / 'meeting'
            folder.mkdir()
            (folder / '.overtalk-owned').write_text('1')
            (folder / 'audio.wav').write_bytes(b'synthetic private file')
            saved = service.call('api_save_transcript', meetingTitle='Synthetic privacy check', folderPath=str(folder),
                transcripts=[dict(id='privacy-line', text='Alice requested review on Thursday.', timestamp='00:00', audio_start_time=0., audio_end_time=2., duration=2.)])
            meeting = saved['meeting_id']
            assert meeting in privacy(op='subject_preview', name='Alice')['ids']
            checks['subject_review_direct_transcript'] = 'passed'
            privacy(op='hold', meeting_id=meeting, reason='Synthetic retention hold')
            rejected(lambda: service.call('api_delete_meeting', meetingId=meeting), 'hold')
            assert folder.exists()
            privacy(op='release_hold', meeting_id=meeting)
            checks['hold_blocks_erasure'] = 'passed'
            auth = privacy(op='authorize_voice', name='Alice', basis='explicit_consent', reference='Synthetic test only', days=1)['id']
            privacy(op='withdraw_authorization', id=auth)
            assert next(a for a in privacy(op='status')['authorizations'] if a['id']==auth)['withdrawn']
            checks['authorization_withdrawal'] = 'passed'
            rejected(lambda: service.call('start_recording'), 'notice')
            checks['recording_requires_notice'] = 'passed'
            archive = root / 'before-erasure.age'
            service.call('workspace_create_backup', path=str(archive), passphrase='synthetic-test-passphrase', includeAudio=False)
            assert archive.read_bytes().startswith(b'age-encryption.org')
            policy = privacy(op='status')['policy']
            policy['local_only'] = True
            privacy(op='save_policy', policy=policy)
            service.call('api_delete_meeting', meetingId=meeting)
            assert not folder.exists()
            checks['permanent_media_erasure'] = 'passed'
            database = root / 'data/com.judeam.sonefold/meeting_minutes.sqlite'
            with sqlite3.connect(database) as db:
                assert db.execute('SELECT COUNT(*) FROM privacy_erasures WHERE meeting_id=?',(meeting,)).fetchone()[0] == 1
                assert db.execute('SELECT COUNT(*) FROM transcripts WHERE meeting_id=?',(meeting,)).fetchone()[0] == 0
            service.call('workspace_schedule_restore', path=str(archive), passphrase='synthetic-test-passphrase')
            service.close()
            service = Service(args.binary.resolve(), root)
            privacy = lambda **action: service.call('workspace_privacy', action=action)
            assert privacy(op='status')['policy']['local_only']
            with sqlite3.connect(database) as db:
                assert db.execute('SELECT COUNT(*) FROM meetings WHERE id=?',(meeting,)).fetchone()[0] == 0
                assert db.execute('SELECT withdrawn_at FROM privacy_authorizations WHERE id=?',(auth,)).fetchone()[0]
            health = privacy(op='verify_database')
            assert health['valid'], health
            assert tuple(map(int, health['sqlite_version'].split('.'))) >= (3,54,1), health
            checks['encrypted_restore_preserves_erasure_policy_and_withdrawal'] = 'passed'
            verified = privacy(op='verify_audit')
            assert verified.get('valid'), verified
            checks['audit_integrity'] = 'passed'
            mode = os.stat(database).st_mode & 0o777
            if os.name != 'nt':
                assert mode & 0o077 == 0, oct(mode)
            checks['private_database_permissions'] = 'passed'
        finally:
            service.close()
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(checks, indent=2)+'\n')
    print(json.dumps(checks, indent=2))


if __name__ == '__main__':
    main()
