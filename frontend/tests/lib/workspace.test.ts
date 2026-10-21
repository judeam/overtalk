import {describe,expect,test} from 'bun:test';
import {sourceUrl,sourceTime,taskEvidence,type MeetingSource} from '../../src/lib/workspace';
const source:MeetingSource={id:'transcript:a',meeting_id:'meeting/?日本語',source_kind:'transcript',source_id:'segment&one',title:'One',body:'Quote',start_time:12.5,end_time:15,recorded_at:'2026-09-30',fingerprint:'f'};
describe('source navigation',()=>{
 test('encodes meeting and segment IDs independently from seek position',()=>{const url=new URL(sourceUrl(source),'https://local');expect(url.searchParams.get('id')).toBe(source.meeting_id);expect(url.searchParams.get('segment')).toBe(source.source_id);expect(url.searchParams.get('at')).toBe('12.5');});
 test('never invents an audio seek position for notes or invalid times',()=>{for(const time of [null,NaN,Infinity,-1])expect(sourceUrl({...source,source_kind:'note',start_time:time})).not.toContain('at=');expect(sourceTime(null)).toBe('Open notes');expect(sourceTime(65)).toBe('1:05');});
 test('corrupt task evidence leaves the action readable',()=>{expect(taskEvidence({evidence_json:'broken'} as any)).toEqual([]);expect(taskEvidence({evidence_json:'[null,{}]'} as any)).toEqual([]);});
});
