#!/usr/bin/env python3
import json,os,sys,time
fixture=json.load(open(os.environ['OVERTALK_FIXTURE'], encoding='utf-8'))
position=0.0
playing=False
started=time.monotonic()
rate=1.0
pads={}
record_starts=0
# OVERTALK_VISUAL_PROJECTS answers with calendar events, automatic filing, suggestions and briefs.
projects_on=os.environ.get('OVERTALK_VISUAL_PROJECTS')=='1'
toast=os.environ.get('OVERTALK_VISUAL_TOAST')=='1'
# OVERTALK_VISUAL_FLOWS checks the arguments of project choices made in the UI.
flows=os.environ.get('OVERTALK_VISUAL_FLOWS')=='1'
# OVERTALK_VISUAL_NOTICE gives meetings a notice record; "external" words the notice for a cloud service.
notice_mode=os.environ.get('OVERTALK_VISUAL_NOTICE','')
LOCAL_NOTICE="I'm recording this call to take notes. It is recorded and transcribed on my computer; nothing is sent to a cloud service."
EXTERNAL_NOTICE="I'm recording this call to take notes. It is recorded on my computer, and {services}."
automation={'auto_assign':'confident','briefs':'projects','reminder_minutes':10}
LAUNCH_REASON='Lena Fischer and Marco Rossi usually attend · Invitees from example.test · Recurring series “Launch review”'
ORION_REASON='Part of the “Orion release review” series · Maya Chen and Jonas Weber usually attend'
def people(*names):return json.dumps([{'name':n,'email':f'{n.split()[0].lower()}@example.test'} for n in names])
# A fixture may bring its own calendar, briefs and projects, as the website's Orion 2 captures do.
memory=fixture.get('project_memory',{})
def upcoming_events():return memory.get('events') or [
 {'id':'call-1','title':'Product review · launch readiness','starts_at':'2026-10-17T10:00:00Z','ends_at':'2026-10-17T10:45:00Z','source':'google:test','participants':people('Lena','Marco'),'meeting_url':'https://meet.google.com/test-call','project':{'id':'launch','name':'Product launch','origin':'automatic','reason':LAUNCH_REASON},'brief':'ready'},
 {'id':'call-2','title':'Orion kickoff with Acme','starts_at':'2026-10-17T13:30:00Z','ends_at':'2026-10-17T14:00:00Z','source':'microsoft:test','participants':json.dumps([{'name':'Maya Chen','email':'maya@acme.example'},{'name':'Lena','email':'lena@example.test'}]),'meeting_url':'https://teams.microsoft.com/l/test','project':{'id':'orion','name':'Orion','origin':'suggested','reason':ORION_REASON},'brief':'facts'},
 {'id':'call-3','title':'Design workshop','starts_at':'2026-10-18T09:00:00Z','ends_at':'2026-10-18T10:00:00Z','source':'local','participants':people('Priya','Lena','Marco'),'meeting_url':'','project':{'id':'launch','name':'Product launch','origin':'manual','reason':''},'brief':None}]
def source(start,label):return {'meeting_id':'m1','start':start,'label':label}
BRIEFS=memory.get('briefs') or {
 'call-1':{'event_id':'call-1','project_id':'launch','project_name':'Product launch','title':'Product review · launch readiness','starts_at':'2026-10-17T10:00:00Z','status':'ready','body':{
  'summary':'The third launch review. Search ships first; export waits on the PDF table layout.',
  'sections':[{'title':'Decided last time','items':[{'text':'Search ships before the export redesign, with a first version at the end of October.','source':source(189,'Product sync · 03:09')}]},
   {'title':'Open actions','items':[{'text':'Priya: rework the PDF tables, two or three days.','source':source(151,'Product sync · 02:31')},{'text':'Marco: write the release note once search is on the test machines.','source':None}]},
   {'title':'Still open','items':[{'text':'Who sends the next customer update?','source':None}]},
   {'title':'Who’s coming','items':[{'text':'Lena Fischer (lena@example.test) · Product lead · last met Mon 12 Oct','source':None},{'text':'Maya Chen (maya@acme.example) · Product lead · last met Tue 13 Oct','source':None}]},
   {'title':'Tickets from earlier meetings','items':[{'text':'ENG-42 · Fix export tables — In progress','source':source(151,'Product sync · 02:31')}]}],
  'agenda':[{'text':'Confirm search is on the test machines','source':source(189,'Product sync · 03:09')},{'text':'Review the PDF table layout with Priya','source':source(151,'Product sync · 02:31')},{'text':'Agree who sends the customer update','source':None}],
  'note':''}},
 'call-2':{'event_id':'call-2','project_id':'orion','project_name':'Orion','title':'Orion kickoff with Acme','starts_at':'2026-10-17T13:30:00Z','status':'facts','body':{
  'summary':'First meeting with Acme in the Orion project.',
  'sections':[{'title':'From earlier Orion meetings','items':[{'text':'Acme asked for an on-premises option.','source':source(72,'Customer feedback · 01:12')}]}],
  'agenda':[],'note':'Talking points are still being prepared. The facts below come from earlier meetings.'}}}
rules=[{'id':'r1','kind':'email','value':'lena@example.test','label':'lena@example.test','origin':'learned','confirmations':6},
 {'id':'r2','kind':'domain','value':'example.test','label':'example.test','origin':'manual','confirmations':0},
 {'id':'r3','kind':'series','value':'series-weekly','label':'Weekly product sync','origin':'learned','confirmations':4},
 {'id':'r4','kind':'title','value':'launch','label':'launch','origin':'manual','confirmations':0},
 {'id':'r5','kind':'account','value':'google-test','label':'Work calendar','origin':'learned','confirmations':1}]
def memory_projects():
 if 'projects' in memory:return memory['projects']
 base=fixture.get('projects_list',{'projects':[],'membership':[]})
 return {'projects':base['projects']+[{'id':'orion','name':'Orion','context':''}],
  'membership':[{'meeting_id':'m3','project_id':'launch','series':'','origin':'automatic','reason':LAUNCH_REASON},{'meeting_id':'m4','project_id':'launch','series':'Weekly product sync','origin':'manual','reason':''}],
  'suggestions':[{'meeting_id':'m1','project_id':'orion','reason':ORION_REASON+' · The transcript mentions Orion 7 times'}]}
def send(v):print('OTRPC1 '+json.dumps(v),flush=True)
send({'kind':'ready','commands':list(fixture)})
for line in sys.stdin:
 if not line.startswith('OTRPC1 '):continue
 request=json.loads(line[7:]);cmd=request['command'];args=request.get('args') or {}
 if cmd=='shutdown':break
 if cmd=='cloud_account':
  assert args['action']['op']=='status', 'Visual fixtures never create a real checkout'
  # OVERTALK_VISUAL_CLOUD: "1" is the legacy credits gateway, "e2e" a summaries subscriber on the end-to-end route,
  # "e2e-offer" someone who has not subscribed yet.
  cloud=os.environ.get('OVERTALK_VISUAL_CLOUD')
  result={'configured':True,'connected':True,'balance':{'available_micro_usd':1580000,'held_micro_usd':80000,'suspended':False}} if cloud=='1' else {'route':'end-to-end','configured':True,'connected':True,'entitled':True,'monthly_hours':200} if cloud=='e2e' else {'route':'end-to-end','configured':True,'connected':False,'entitled':False,'monthly_hours':200} if cloud=='e2e-offer' else {'configured':False,'connected':False}
 elif cmd=='workspace_join_event':
  assert args['id']=='call-1'
  result=None
 elif cmd=='start_recording_with_devices_and_meeting':
  record_starts+=1
  assert record_starts==1, 'Duplicate Join & take notes started two recordings'
  assert args['workflow']['event_id']=='call-1'
  assert args['workflow']['auto_notes'] is True
  assert args['meetingName']=='Product review · launch readiness', args['meetingName']
  assert args['workflow']['link_event'] is True
  capture={'meeting_id':'capture-test','project_id':None,'project_name':None,'project_origin':'undecided','project_reason':'','event_title':args['meetingName']}
  if flows:
   assert args['workflow'].get('project_id')=='orion' and 'project_mode' not in args['workflow'], 'The recorder choice files the recording'
   capture.update(project_id='orion',project_name='Orion',project_origin='manual')
  else:
   assert 'project_id' not in args['workflow'] and 'project_mode' not in args['workflow'], 'The recorder default leaves filing to the engine'
   if projects_on:capture.update(project_id='launch',project_name='Product launch',project_origin='automatic',project_reason=LAUNCH_REASON)
  result={'recording_id':'capture-test','capture':capture}
 elif cmd=='workspace_edition':
  result={'name':'open','capabilities':['auto-assign','briefs'] if projects_on else []}
 elif cmd=='workspace_workflow':
  action=args['action'];op=action['op']
  if op=='dashboard' and projects_on:
   if toast:
    toast=False
    send({'kind':'event','event':'workspace-brief-ready','payload':{'event_id':'call-1','title':'Product review · launch readiness','starts_at':'2026-10-17T10:00:00Z'}})
   result={'accounts':[{'id':'google-test','provider':'google','name':'Work calendar'}],'events':upcoming_events(),'sync':[]}
  elif op=='brief':result=BRIEFS.get(action['event_id'])
  elif op=='automation':
   if action.get('settings'):automation=action['settings']
   result=automation
  elif op=='capture_project':
   assert action['meeting_id']=='capture-test' and action['project_id'] in ['launch','orion',None]
   project=next((p for p in memory_projects()['projects'] if p['id']==action['project_id']),None)
   result={'meeting_id':action['meeting_id'],'project_id':action['project_id'],'project_name':project and project['name'],'project_origin':'manual','project_reason':'','event_title':None}
  elif op=='dashboard':
   result={'accounts':[{'id':'google-test','provider':'google','name':'Work calendar'}],'events':[{'id':'call-1','title':'Product review · launch readiness','starts_at':'2026-10-17T10:00:00Z','ends_at':'2026-10-17T10:45:00Z','source':'google:test','participants':json.dumps([{'name':'Lena','email':'lena@example.test'},{'name':'Marco','email':'marco@example.test'}]),'meeting_url':'https://meet.google.com/test-call'}],'sync':[]}
  elif op=='load':result=pads.get(action['id'],memory.get('pad') or {'body':'Questions to cover:\n• Confirm the launch scope\n• Who owns the next customer update?','revision':0,'jobs':[]})
  elif op=='save':
   old=pads.get(action['id'],{'revision':0})
   assert old['revision']==action['revision'], 'Stale personal notes save'
   result={'revision':action['revision']+1};pads[action['id']]={**result,'body':action['body'],'jobs':[]}
  elif op=='readiness' and os.environ.get('OVERTALK_VISUAL_SETUP')=='downloading':
   # The first run: both models still downloading.
   result={'speech':None,'speech_error':'Model is not downloaded','speech_progress':34,'notes_ready':False,'notes_configured':False,'notes_progress':12,'model':'qwen3.5:2b','model_label':'Qwen 3.5 2B'}
  elif op=='readiness':result={'speech':{'ready':True},'notes_ready':True,'notes_configured':True,'model':'qwen3.5:4b'}
  elif op=='first_run_downloads':result={'speech':{'model':'parakeet-tdt-0.6b-v3-int8','label':'Parakeet TDT 0.6B v3','size_mb':670,'state':'downloading'},'notes':{'model':'qwen3.5:2b','label':'Qwen 3.5 2B','size_mb':1221,'state':'downloading'},'started':['speech','notes']}
  else:result={'body':'','revision':0,'jobs':[]}
 elif cmd=='workspace_ask':
  assert args['request']['meeting_id']=='m1'
  result={'id':'answer-1','claims':[{'text':'The launch review is scheduled for Friday.','citations':[{'id':'source-1','quote':'Friday'}]}],'sources':[{'id':'source-1','title':'Product review','body':'The launch review is scheduled for Friday.','start_time':24.0,'meeting_id':'m1'}]}
 elif cmd=='workspace_tasks':
  result=[{'id':'task-design','title':'Verify the export tables','owner':'Priya','due_date':'2026-10-19','status':'open','evidence':[]},{'id':'task-search','title':'Ship search improvements','owner':'Marco','due_date':'2026-10-16','status':'done','evidence':[]}]
 elif cmd=='workspace_calendar_events':
  result=[{'id':'call-1','title':'Product review · launch readiness','starts_at':'2026-10-17T10:00:00Z','ends_at':'2026-10-17T10:45:00Z','source':'google:test','location':'Google Meet','participants':json.dumps([{'name':'Lena','email':'lena@example.test'},{'name':'Marco','email':'marco@example.test'}]),'meeting_url':'https://meet.google.com/test-call'}, {'id':'in-person','title':'Design workshop','starts_at':'2026-10-18T09:00:00Z','ends_at':'2026-10-18T10:00:00Z','source':'local','location':'Studio','participants':'[]','meeting_url':''}]
 elif cmd=='workspace_backup_settings':
  result={'enabled':False,'directory':'','interval_hours':24,'include_audio':False}
 elif cmd=='usage_report':
  result={'format':'overtalk-usage-v1','edition':'open','first_opened':'2026-10-20','weeks_with_meetings':2,
   'totals':{'app_opened':6,'recording_started':5,'transcript_ready':5,'notes_ready':4,'source_played':12,'export_saved':1},
   'first_run':{'minutes_from_first_open_to_first_recording':4,'model_download_minutes':9,'minutes_from_first_recording_to_first_transcript':11,'minutes_from_first_recording_to_first_notes':13,'days_from_first_open_to_first_brief_with_a_source':None},
   'weeks':[],'contains':'Counts and dates only.'}
 elif cmd=='usage_record':result=None
 elif cmd=='workspace_share_estimate':
  options=args['options']
  assert args['meetingId']=='m1' and options['kind'] in ['page','file'], args
  assert options['audio'] in (['clips','full','none'] if options['kind']=='page' else ['original','compact','none']), options
  page=options['kind']=='page'
  result={'bytes':1468006 if page else 5767168,'clips':4 if page and options['audio']=='clips' else 0,'audio_seconds':52.0,'notes':not options['redactions']['spans']}
 elif cmd=='workspace_privacy':
  op=args['action']['op']
  if op=='status':
   result={'policy':{'local_only':True,'protected_mode':False,'require_recording_notice':True,'recording_notice_local':LOCAL_NOTICE,'recording_notice_external':EXTERNAL_NOTICE,'retention_days':90,'automatic_retention':False,'audit_retention_days':90,'diagnostic_retention_days':7,'approved_hosts':[],'organization':'Example team','privacy_contact':'privacy@example.test','recording_basis':'Participants informed before recording','agreement_reference':''},'storage':[{'path':'/synthetic/library','verified':False,'detail':'Not verified in fixture'}],'pending_deletions':0,'pending_credentials':0,'authorizations':[],'retention_candidates':[],'holds':[],'meetings':[{'id':'m1','title':'Product review'}]}
  elif op=='recording_notice':
   assert isinstance(args['action'].get('auto_notes'), bool), 'The wording depends on automatic notes'
   services='the transcript is encrypted before it is sent to Privatemode to write the notes'
   result={'text':EXTERNAL_NOTICE.replace('{services}',services),'route':'external','detail':f'Your setup: {services}.','required':True} if notice_mode=='external' else {'text':LOCAL_NOTICE,'route':'local','detail':'Your setup: speech and notes stay on this computer.','required':True}
  elif op=='acknowledge_recording':
   assert args['action'].get('method') in ['copied','read_aloud','skipped',None], args['action']
   assert args['action'].get('method') is None or isinstance(args['action'].get('auto_notes'), bool), args['action']
   result={'acknowledged':True}
  elif op=='verify_audit':result={'valid':True}
  elif op=='audit':result=[]
  else:result={'saved':True}
 elif cmd=='workspace_project':
  op=args.get('action',{}).get('op')
  if projects_on and op=='get':result={**fixture.get('project_snapshot',{}),'rules':rules}
  elif projects_on and op=='list':result=memory_projects()
  elif projects_on and op=='rule':
   action=args['action']
   if action['kind']=='email' and '@' not in action['value']:
    send({'kind':'reply','id':request['id'],'result':None,'error':'Enter an email address'});continue
   rules.append({'id':f'r{len(rules)+1}','kind':action['kind'],'value':action['value'],'label':action['value'],'origin':'manual','confirmations':0});result={'id':rules[-1]['id']}
  elif projects_on and op=='event_project':
   action=args['action']
   assert action['event_id'] in ['call-1','call-2','call-3'] and isinstance(action['automatic'],bool)
   assert not action['automatic'] or action['project_id'] is None, 'Deciding automatically clears the choice'
   result=None
  elif projects_on and op in ['accept_suggestion','dismiss_suggestion']:
   assert args['action']['meeting_id']=='m1';result=None
  elif projects_on and op=='assign':
   assert args['action']['series'] is None, 'A meeting choice keeps its series label';result=None
  elif projects_on and op=='remove_rule':
   rules[:]=[r for r in rules if r['id']!=args['action']['id']];result=None
  elif projects_on:result=None
  else:result=fixture.get('project_snapshot',{}) if op=='get' else fixture.get('projects_list',{'projects':[],'membership':[]})
 elif cmd=='workspace_review':result=fixture.get('project_issues',[]) if args.get('action',{}).get('op')=='list' else []
 elif cmd=='workspace_jobs':result=fixture.get('project_jobs',[])
 elif cmd=='workspace_account_auth':result={'google_configured':False,'providers':[{'id':p,'configured':p in ['linear_mcp','atlassian_mcp','notion_mcp']} for p in ['google','microsoft','slack','notion_mcp','linear_mcp','atlassian_mcp','github']]}
 elif cmd=='workspace_connections':
  if os.environ.get('OVERTALK_VISUAL_SEND'):
   op=args['action']['op']
   if op=='list':result=[{'id':'slack-test','name':'Design team','description':'slack','status':'Account linked'}]
   elif op=='browse':result=[{'id':'C_DESIGN','name':'#design-review'}]
   elif op=='draft':
    assert args['action']['target']=='C_DESIGN' and args['action']['body'] and args['action']['title']
    result={'id':'review-test','hash':'verified-draft','connection_name':'Design team','target':args['action']['target'],'title':args['action']['title'],'body':args['action']['body']}
   else:raise AssertionError('No publishing is authorized by the visual workflow test')
  else:result=fixture.get('connections',[])
 elif cmd=='workspace_meeting_states':result=[]
 elif cmd=='api_list_templates':result=[{'id':'daily_standup','name':'Meeting notes'}]
 elif cmd in ['speech_get_settings','library_get_settings']:result={}
 elif cmd=='integrations_get_settings':result=fixture.get('integrations',{})
 elif cmd=='voice_memory_status':result={'ready':True,'profiles':[{'id':'voice-lena','name':'Lena','created_at':'2026-10-15'}]}
 elif cmd=='list_meeting_speakers':result=[dict(s,segment_count=12,identity_source='matched' if i==0 else None) for i,s in enumerate(fixture['meeting_timeline']['speakers'])]
 elif cmd=='native_audio':
  now=time.monotonic()
  if playing:position+=(now-started)*rate
  started=now
  action=args.get('action')
  if action in ['load','play','seek']:position=float(args.get('value',0))
  if action in ['load','play']:playing=True
  if action=='pause':playing=False
  if action=='stop':playing=False;position=0.0
  if action=='speed':rate=float(args.get('value',1))
  result={'playing':playing,'position':position}
 elif cmd=='api_get_meeting_transcripts':
  data=fixture[cmd];offset=int(args.get('offset',0));limit=int(args.get('limit',100));rows=data['transcripts'];result={**data,'transcripts':rows[offset:offset+limit],'total_count':len(rows),'has_more':offset+limit<len(rows)}
 elif cmd=='get_audio_devices':result=fixture.get(cmd,[])
 elif cmd=='api_get_meeting_metadata' and notice_mode:
  result={**fixture[cmd],'recording_notice':'Participants were notified at 10:02 (notice copied to chat)'}
 elif cmd in fixture:result=fixture[cmd]
 else:result=None
 send({'kind':'reply','id':request['id'],'result':result,'error':None})
