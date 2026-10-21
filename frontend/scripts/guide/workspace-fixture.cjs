// Synthetic native responses for the responsive workspace and its UI regression checks.
function supplement(opts = {}){
 const original=window.__TAURI_INTERNALS__.invoke;
 window.__auditCalls=[];
 window.__TAURI_INTERNALS__.invoke=async function(cmd,args){
  window.__auditCalls.push({cmd,args});
  if(opts.audioFailure&&cmd==='workspace_audio_source') throw Error('Recording file not found');
  if(cmd==='get_audio_backend_info')return [{id:'pulseaudio',name:'PulseAudio',description:'Desktop audio capture'}];
  if(cmd==='get_current_audio_backend')return 'pulseaudio';
  if(cmd==='get_default_recordings_folder_path')return '/home/demo/Recordings';
  if(cmd==='get_database_directory')return '/home/demo/.local/share/sonefold';
  if(cmd==='whisper_get_models_directory')return '/home/demo/.local/share/sonefold/models';
  if(cmd==='builtin_ai_get_recommended_model')return 'qwen3.5:4b';
  if(cmd==='library_get_settings')return {enabled:false,python:'',device:'auto',family:'nemotron3',size:'8B',reranker_size:'4B',embedding_dir:'',reranker_dir:'',rerank:true};
  if(cmd==='integrations_get_settings')return {settings:{api_enabled:false,api_port:8421,api_token:'',cli_token:'',mcp_token:'',record_control:false,write_notes:false,cli_enabled:false,mcp_enabled:false,read_meetings:false,read_transcripts:false,read_summaries:false,webhook_enabled:false,webhook_url:'',webhook_secret:'',webhook_include_summary:false},config_path:'/home/demo/.local/share/sonefold/connections.json',companion_path:'/opt/sonefold/companion.py'};
  if(cmd==='speech_get_settings')return {enabled:false,python:'',diarization_python:'',device:'auto',model:'qwen3-asr-1.7b',diarization_model:'community-1',asr_model_dir:'',aligner_model_dir:'',diarization_model_path:'',live_diarization:false,streaming_asr_model:'',streaming_asr_model_path:''};
  if(cmd==='get_local_quality_profile')return {speech_model:'qwen3-asr-1.7b'};
  return original(cmd,args);
 };
}
function updatedFixture(){
 const original=window.__TAURI_INTERNALS__.invoke;const states=new Map();const corrections=new Map();let summary=null;
 window.__TAURI_INTERNALS__.invoke=async function(cmd,args){
  if(cmd==='workspace_meeting_states')return [...states.values()];
  if(cmd==='workspace_organise_meeting'){const c=args.change;states.set(c.meeting_id,{meeting_id:c.meeting_id,title:'Product sync: Q4 roadmap',pinned:false,archived:false,...states.get(c.meeting_id),...c,trashed_at:c.trashed?'2026-10-15':null});return;}
  if(cmd==='workspace_correct_transcript'){corrections.set(args.change.segment_id,args.change);return;}
  if(cmd==='api_get_meeting_transcripts'){const r=await original(cmd,args);return {...r,transcripts:r.transcripts.map(t=>{const c=corrections.get(t.id);return c?{...t,text:c.text,speaker_id:c.speaker_id}:t;})};}
  if(cmd==='api_save_meeting_summary'){summary=args.summary;window.__savedNotes=summary;return;}
  if(cmd==='api_get_summary'&&summary){const r=await original(cmd,args);return {...r,data:summary};}
  if(cmd==='workspace_source_at')return {id:'transcript:t2',meeting_id:'m1',source_kind:'transcript',source_id:'t2',title:'Product sync: Q4 roadmap',body:'I can take the PDF tables, two or three days.',start_time:151,end_time:160,recorded_at:'2026-10-12',fingerprint:'fixture'};
  if(cmd==='workspace_search'&&args.request.query)return {mode:'full-text',warnings:[],sources:[{id:'transcript:t1',meeting_id:'m1',source_kind:'transcript',source_id:'t1',title:'Product sync: Q4 roadmap',body:'Export is blocked on the PDF layout for long tables.',start_time:110,end_time:120,recorded_at:'2026-10-12',fingerprint:'fixture'}]};
  return original(cmd,args);
 };
}
module.exports = { supplement, updatedFixture };
