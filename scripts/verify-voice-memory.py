#!/usr/bin/env python3
"""Verify voice memory and external-write refresh in a disposable app profile.

The repeated audio is deliberately identical: this tests native wiring, not
speaker-recognition accuracy. Quality is measured separately on held-out speech.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import time

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('binary',type=Path)
p.add_argument('--model',type=Path,required=True)
p.add_argument('--audio',type=Path,required=True)
args=p.parse_args()
with tempfile.TemporaryDirectory(prefix='overtalk-voice-test-') as temporary:
    root=Path(temporary);data=root/'data/com.judeam.sonefold';models=data/'models/voice-memory'
    models.mkdir(parents=True);shutil.copyfile(args.model,models/'campplus.onnx')
    env=dict(os.environ,XDG_DATA_HOME=str(root/'data'),XDG_CONFIG_HOME=str(root/'config'),XDG_CACHE_HOME=str(root/'cache'))
    env.setdefault('DISPLAY',':1');env.setdefault('DBUS_SESSION_BUS_ADDRESS',f'unix:path=/run/user/{os.getuid()}/bus')
    q=queue.Queue();events=[];serial=0
    with (root/'engine.log').open('w') as log:
        process=subprocess.Popen([str(args.binary.resolve()),'--stdio'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,env=env)
        def receive():
            for line in process.stdout:
                if line.startswith(b'OTRPC1 '):
                    item=json.loads(line[7:]);q.put(item)
                    if item.get('kind')=='event':events.append(item)
        threading.Thread(target=receive,daemon=True).start()
        def call(command,**kwargs):
            global serial
            serial+=1;process.stdin.write(b'OTRPC1 '+json.dumps(dict(id=serial,command=command,args=kwargs)).encode()+b'\n');process.stdin.flush()
            while True:
                item=q.get(timeout=90)
                if item.get('id')==serial:
                    if item.get('error'):raise RuntimeError(item['error'])
                    return item['result']
        try:
            while q.get(timeout=90).get('kind')!='ready':pass
            assert call('voice_memory_status')==dict(ready=True,profiles=[])
            for name in ['Test speaker','Unsafe overlap','Alex']:
                call('workspace_privacy',action=dict(op='authorize_voice',name=name,basis='explicit_consent',reference='Synthetic verification fixture',days=1))
            call('voice_memory_setup') # verifies existing model, no download
            folder=root/'meeting';folder.mkdir();shutil.copyfile(args.audio,folder/'audio.wav')
            meetings=[call('api_save_transcript',meetingTitle=name,folderPath=str(folder),transcripts=[])['meeting_id'] for name in ['Enrollment','Later meeting','Overlapping voices']]
            db=data/'meeting_minutes.sqlite'
            with sqlite3.connect(db) as sql:
                sql.execute('PRAGMA foreign_keys=ON')
                for meeting in meetings:
                    sql.execute('INSERT INTO meeting_speakers VALUES(?,?,?)',(meeting,'A','Speaker 1'))
                    for start,end in [(1,4.4),(5,8.4),(9,12.4)]:
                        sql.execute('INSERT INTO speaker_turns VALUES(?,?,?,?,?,?)',(meeting,'A',start,end,1,'fixture'))
                sql.execute('INSERT INTO meeting_speakers VALUES(?,?,?)',(meetings[2],'B','Speaker 2'))
                sql.execute('INSERT INTO speaker_turns VALUES(?,?,?,?,?,?)',(meetings[2],'B',0,20,1,'fixture'))
            start=time.perf_counter();call('remember_speaker',meetingId=meetings[0],speakerId='A',name='Test speaker')
            print(f'PASS: enroll verified local voice in {time.perf_counter()-start:.3f}s')
            status=call('voice_memory_status');assert len(status['profiles'])==1
            assert 'embeddings' not in status['profiles'][0]
            assert call('recognize_meeting_speakers',meetingId=meetings[1])==1
            speaker=call('list_meeting_speakers',meetingId=meetings[1])[0]
            assert speaker['name']=='Test speaker' and speaker['identity_source']=='matched'
            print('PASS: later meeting recognizes remembered voice with provenance')
            try:call('remember_speaker',meetingId=meetings[2],speakerId='A',name='Unsafe overlap')
            except RuntimeError as error:assert 'two clear speech samples' in str(error)
            else:raise AssertionError('Overlapping audio was enrolled')
            call('rename_meeting_speaker',meetingId=meetings[1],speakerId='A',name='Correction')
            assert call('list_meeting_speakers',meetingId=meetings[1])[0]['identity_source'] is None
            call('forget_voice',profileId=status['profiles'][0]['id'])
            assert call('voice_memory_status')['profiles']==[]
            assert call('recognize_meeting_speakers',meetingId=meetings[1])==0
            print('PASS: cross-talk rejected, correction detaches match, deletion removes profile')
            revoked = call('workspace_privacy',action=dict(op='authorize_voice',name='Withdraw speaker',basis='explicit_consent',reference='Synthetic withdrawal test',days=1))['id']
            call('remember_speaker',meetingId=meetings[0],speakerId='A',name='Withdraw speaker')
            call('workspace_privacy',action=dict(op='withdraw_authorization',id=revoked))
            assert call('voice_memory_status')['profiles']==[]
            assert call('recognize_meeting_speakers',meetingId=meetings[1])==0
            expires = call('workspace_privacy',action=dict(op='authorize_voice',name='Expiry speaker',basis='explicit_consent',reference='Synthetic expiry test',days=1))['id']
            call('remember_speaker',meetingId=meetings[0],speakerId='A',name='Expiry speaker')
            expired_profile=call('voice_memory_status')['profiles'][0]['id']
            with sqlite3.connect(db) as sql:
                sql.execute("UPDATE privacy_authorizations SET expires_at=datetime('now','-1 day') WHERE id=?",(expires,))
            assert call('recognize_meeting_speakers',meetingId=meetings[1])==0
            call('forget_voice',profileId=expired_profile)
            print('PASS: withdrawn and expired authorization immediately prevent voice recognition')
            project_ids = [call('workspace_project', action=dict(op='save', name=name, context='Synthetic voice scope'))['id'] for name in ['Voice project A','Voice project B']]
            people = [call('workspace_project', action=dict(op='person', project_id=project, name='Alex', role='Synthetic test'))['id'] for project in project_ids]
            call('workspace_project', action=dict(op='assign', meeting_id=meetings[0], project_id=project_ids[0]))
            later=call('api_save_transcript',meetingTitle='Project voice check',folderPath=str(folder),transcripts=[])['meeting_id']
            with sqlite3.connect(db) as sql:
                sql.execute('INSERT INTO meeting_speakers VALUES(?,?,?)',(later,'A','Speaker 1'))
                for start,end in [(1,4.4),(5,8.4),(9,12.4)]:
                    sql.execute('INSERT INTO speaker_turns VALUES(?,?,?,?,?,?)',(later,'A',start,end,1,'fixture'))
            call('workspace_project', action=dict(op='assign', meeting_id=later, project_id=project_ids[1]))
            call('remember_speaker',meetingId=meetings[0],speakerId='A',name='',personId=people[0])
            assert call('recognize_meeting_speakers',meetingId=later)==0, 'A voice leaked into a different project'
            call('workspace_project', action=dict(op='assign', meeting_id=later, project_id=project_ids[0]))
            assert call('recognize_meeting_speakers',meetingId=later)==1
            assert call('list_meeting_speakers',meetingId=later)[0]['name']=='Alex'
            call('workspace_project',action=dict(op='remove_person',id=people[0]))
            assert call('voice_memory_status')['profiles']==[]
            print('PASS: roster voice matches only within its project; removing the person deletes its voice profile')
            time.sleep(.8);events.clear()
            with sqlite3.connect(db) as sql:sql.execute('UPDATE meetings SET title=? WHERE id=?',('External write',meetings[0]))
            deadline=time.monotonic()+3
            while time.monotonic()<deadline and not any(e['event']=='library-data-changed' for e in events):time.sleep(.02)
            assert any(e['event']=='library-data-changed' for e in events),'External database write did not trigger refresh'
            time.sleep(.6);events.clear();time.sleep(.6)
            assert not any(e['event']=='library-data-changed' for e in events),'Watcher emitted an idle update loop'
            print('PASS: external SQLite changes trigger refresh; no idle event loop')
        except Exception:
            print((root/'engine.log').read_text()[-3000:]);raise
        finally:
            try:
                process.stdin.write(b'OTRPC1 {"id":999999,"command":"shutdown","args":{}}\n');process.stdin.flush()
                process.wait(timeout=20)
            except Exception:process.terminate();process.wait(timeout=20)
