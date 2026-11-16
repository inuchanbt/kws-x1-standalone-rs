"""Decode the committed RAM trace in CAPFLASH.BIN and align it with CY4500."""
import argparse
import csv
import json
import struct
from pathlib import Path

MAGIC, COMMIT = 0x54543158, 0x4B4F5458
EVENTS = {1:'start',2:'capture_request',3:'prepare_begin',4:'prepare_end',5:'save_begin',6:'save_end',7:'end',8:'missed_trigger',9:'sniffer_stopped'}
NAMES = {(0,2,9):'EPR_REQUEST',(0,1,2):'REQUEST',(0,0,3):'ACCEPT',(0,0,6):'PS_RDY'}

def decode_runs(data):
    if len(data) != 6*8192:
        raise ValueError('Expected the complete 48 KiB CAPFLASH.BIN snapshot')
    runs=[]
    for offset in range(0,len(data),8192):
        block=data[offset:offset+8192]
        h=list(struct.unpack_from('<16I',block))
        if h[:4] != [MAGIC,1,MAGIC^0xffffffff,0xfffffffe]:continue
        if struct.unpack_from('<I',block,8188)[0] != COMMIT:
            raise ValueError(f'Uncommitted/interrupted test sector at {0x62000+offset:#x}')
        if h[4]>128 or h[5]>2:raise ValueError('Invalid record count or test mode')
        used=64+16*h[4]; checksum=0x811c9dc5
        for byte in block[:60]+block[64:used]:checksum=((checksum^byte)*0x01000193)&0xffffffff
        if checksum!=h[15]:raise ValueError('Trace checksum mismatch')
        events=[]
        for at in range(64,used,16):
            ms,meta,a,b=struct.unpack_from('<4I',block,at)
            kind,header=meta>>16,meta&65535
            if kind>=0x100:
                count=(header>>12)&7
                name=NAMES.get((header>>15,count,header&31),'OTHER_PD')
                events.append({'ms':ms,'event':name,'kind':kind,'header':header,'msg_id':(header>>9)&7,'data':struct.pack('<II',a,b)[:count*4].hex(),'truncated':bool(kind&0x8000)})
            else:
                events.append({'ms':ms,'event':EVENTS.get(meta,'unknown'),'id':a,'value':b})
        runs.append({'sector':hex(0x62000+offset),'mode':['baseline','screen','tall'][h[5]],'start_ms':h[6],'end_ms':h[7],'total_received':h[8],'goodcrc_count':h[9],'keepalive_count':h[10],'dropped_trace_events':h[11],'requests':h[12],'saved':h[13],'save_failures':h[14],'events':events})
    return runs


def decode(data):
    runs=decode_runs(data)
    if len(runs)!=1:raise ValueError(f'Expected exactly one test run, found {len(runs)}; export each run separately and explicitly clear before the next')
    return runs[0]

def compare(run, cy_rows):
    critical={'EPR_REQUEST','REQUEST','ACCEPT','PS_RDY'}
    kws=[e for e in run['events'] if e['event'] in critical and e['kind']==0x107]
    cy=[r for r in cy_rows if r['SOP']=='SOP' and r['Message'] in critical]
    ksig=[(e['event'],e['msg_id'],e['data']) for e in kws]
    csig=[(r['Message'],int(r['Msg Id']),r['Data'].replace(' ','').lower()) for r in cy]
    if ksig != csig:raise ValueError('KWS and CY4500 critical-packet sequences differ; do not claim complete reception')
    anchors=[(int(r['End Time'])/1000,e['ms']) for e,r in zip(kws,cy) if e['event'] in ('EPR_REQUEST','REQUEST')]
    if len(anchors)<2:raise ValueError('At least two matching requests are needed for clock alignment')
    x,y=zip(*anchors);mx=sum(x)/len(x);my=sum(y)/len(y)
    den=sum((v-mx)**2 for v in x)
    if not den:raise ValueError('Request timestamps do not span time')
    slope=sum((a-mx)*(b-my) for a,b in anchors)/den;offset=my-slope*mx
    residual=max(abs(b-(offset+slope*a)) for a,b in anchors)
    uncertainty=20+residual
    windows=[];pending=None
    for r in cy:
        if r['Message'] in ('EPR_REQUEST','REQUEST'):pending=int(r['Start Time'])/1000
        elif r['Message']=='PS_RDY' and pending is not None:
            windows.append((offset+slope*pending,offset+slope*int(r['End Time'])/1000));pending=None
    captures={}
    for e in run['events']:
        if e['event'] in EVENTS.values() and e.get('id',0) and e['event'] not in ('start','end'):
            captures.setdefault(e['id'],{})[e['event']]=e
    intervals=[]
    for ident,events in sorted(captures.items()):
        item={'id':ident,'markers_complete':all(k in events for k in ['capture_request','prepare_begin','prepare_end','save_begin','save_end'])}
        for label,start,end in [('preparation','prepare_begin','prepare_end'),('saving','save_begin','save_end')]:
            if start in events and end in events:
                a,b=events[start]['ms'],events[end]['ms']
                overlap=[max(0,min(b,w1)-max(a,w0)) for w0,w1 in windows]
                item[label+'_ms']=b-a
                item[label+'_overlap_ms']=max(overlap,default=0)
                item[label+'_overlap_supported']=residual<=10 and 0.99<slope<1.01 and any(min(b,w1-uncertainty)>max(a,w0+uncertainty) for w0,w1 in windows)
        item['save_status']=events.get('save_end',{}).get('value')
        item['save_status_name']={2:'saved',3:'failed',4:'full',5:'unavailable',6:'too_large',7:'report_saved'}.get(item['save_status'],'incomplete')
        intervals.append(item)
    errors=[]
    if run['dropped_trace_events']:errors.append('Trace capacity exceeded')
    if run['save_failures']:errors.append(f"{run['save_failures']} capture attempt(s) failed")
    full_ids=[c['id'] for c in intervals if c['save_status']==4]
    if full_ids:errors.append(f'Capture storage was FULL for request IDs {full_ids}')
    if run['requests']!=8:errors.append('Expected eight voltage-change requests')
    if any(e['event'] in ('sniffer_stopped','missed_trigger') or e.get('truncated') for e in run['events']):errors.append('Sniffer stop, missed trigger, or truncated PD evidence')
    expected_status=7 if run['mode']=='tall' else 2
    if run['mode']!='baseline' and (len(intervals)!=8 or any(not c['markers_complete'] or c['save_status']!=expected_status for c in intervals)):
        errors.append('Expected eight successful captures with complete timing markers')
    if run['mode']=='baseline' and intervals:errors.append('Capture activity occurred in baseline')
    if residual>10 or not 0.99<slope<1.01:errors.append('Clock alignment is uncertain')
    attempted_overlap=sum(c.get('preparation_overlap_supported',False) or c.get('saving_overlap_supported',False) for c in intervals)
    covered=sum(c['markers_complete'] and c['save_status']==expected_status and (c.get('preparation_overlap_supported',False) or c.get('saving_overlap_supported',False)) for c in intervals)
    if run['mode']!='baseline' and covered!=8:errors.append(f'Only {covered}/8 captures have supported negotiation overlap')
    return {'critical_packets_matched':len(kws),'request_anchors':len(anchors),'clock_scale':slope,'clock_offset_ms':offset,'request_fit_max_residual_ms':residual,'overlap_guard_ms':uncertainty,'captures':intervals,'capture_attempts_with_supported_overlap':attempted_overlap,'captures_with_supported_overlap':covered,'errors':errors,'status':'review_required' if errors else 'valid_trace','limitation':'Only selective PD events are individually logged. Overlap uses an affine clock fit and a 20 ms + fit-residual guard; this is not independent clock synchronization or a full packet-loss certification.'}


def build_report(data, cy, expected_mode=None):
    """Keep failures visible; select among old traces only using unique PD evidence."""
    report={'expected_mode':expected_mode,'run':None,'selection':{'available_runs':[],'selected_sector':None}}
    try:
        runs=decode_runs(data)
    except ValueError as error:
        report['comparison']={'status':'review_required','errors':[str(error)],'captures':[]}
        return report
    evaluated=[]
    candidates=[]
    for run in runs:
        try:result=compare(run,cy)
        except ValueError as error:result={'status':'review_required','errors':[str(error)],'captures':[]}
        if expected_mode and run['mode']!=expected_mode:
            result['status']='review_required'
            result['errors'].insert(0,f"Device trace mode is {run['mode']}; this sweep expects {expected_mode}. Device mode resets to BASE after power loss. Select the mode after connecting the measurement wiring and confirm the matching RUNNING label before starting the sweep.")
        evaluated.append((run,result))
        report['selection']['available_runs'].append({
            'sector':run['sector'],'mode':run['mode'],'requests':run['requests'],
            'saved':run['saved'],'save_failures':run['save_failures'],
            'matched_critical_packets':result.get('critical_packets_matched'),
            'request_fit_max_residual_ms':result.get('request_fit_max_residual_ms'),
            'status':result['status'],'errors':result['errors'],
        })
        if (not expected_mode or run['mode']==expected_mode) and run['requests']==8 and result.get('critical_packets_matched')==24 and result.get('request_fit_max_residual_ms',float('inf'))<=10 and 0.99<result.get('clock_scale',0)<1.01:
            candidates.append(len(evaluated)-1)
    if len(evaluated)==1:
        selected=0
        report['selection']['method']='only committed trace; validation errors remain authoritative'
    elif len(candidates)==1:
        selected=candidates[0]
        report['selection']['method']='unique expected mode, ordered 24 PD signatures, and request timing fit; capture failures do not exclude a trace'
    else:
        report['selection']['method']='no unique selection'
        report['comparison']={'status':'review_required','errors':[f'Found {len(runs)} committed traces with {len(candidates)} matching candidates; no unique same-run trace can be selected. Raw evidence is retained.'],'captures':[]}
        return report
    report['run'],report['comparison']=evaluated[selected]
    report['selection']['selected_sector']=report['run']['sector']
    return report

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('snapshot',type=Path);p.add_argument('--cy',required=True,type=Path);p.add_argument('--out',type=Path)
    p.add_argument('--expected-mode',choices=['baseline','screen','tall'])
    a=p.parse_args()
    with a.cy.open(encoding='utf-8-sig',newline='') as f:cy=list(csv.DictReader(f))
    report=build_report(a.snapshot.read_bytes(),cy,a.expected_mode)
    run=report['run'] or {};result=report['comparison'];text=json.dumps(report,indent=2)
    if a.out:
        a.out.write_text(text+'\n',encoding='utf-8')
        with a.out.with_name('capture-trace.csv').open('w',encoding='utf-8',newline='') as f:
            writer=csv.DictWriter(f,fieldnames=['boot_ms','event','capture_id','value','message_id','header','data','truncated'])
            writer.writeheader()
            for e in run.get('events',[]):
                writer.writerow({'boot_ms':e['ms'],'event':e['event'],'capture_id':e.get('id',''),'value':e.get('value',''),'message_id':e.get('msg_id',''),'header':f"0x{e['header']:04x}" if 'header' in e else '', 'data':e.get('data',''),'truncated':e.get('truncated','')})
    print(json.dumps({'expected_mode':a.expected_mode,'mode':run.get('mode'),'selected_sector':run.get('sector'),'committed_traces':len(report['selection']['available_runs']),'requests':run.get('requests'),'saved':run.get('saved'),'save_failures':run.get('save_failures'),'status':result['status'],'matched_critical_packets':result.get('critical_packets_matched'),'overlap_count':result.get('captures_with_supported_overlap'),'errors':result['errors'],'report':str(a.out) if a.out else None},indent=2))
    return int(bool(result['errors']))

if __name__=='__main__':raise SystemExit(main())
