import struct
import unittest
import contextlib
import csv
import io
import json
from pathlib import Path
import tempfile
from unittest.mock import patch
from analyze import MAGIC,COMMIT,decode,compare,build_report,main
from check_clear import assess

def fixture(mode=1,delay=30,dropped=0):
    events=[(100,1,mode,0)];cy=[]
    for i in range(8):
        at=1000+i*5000;ident=i+1;header=0x2009|((i%8)<<9)
        data=struct.pack('<II',0xb046e00a+i,0xd7c096f0)
        for name,h,stamp,payload in [('EPR_REQUEST',header,at+1,data),('ACCEPT',3|((i%8)<<9),at+3,b''),('PS_RDY',6|((i%8)<<9),at+666,b'')]:
            a,b=struct.unpack('<II',(payload+b'\0'*8)[:8])
            events.append((stamp+100,0x107<<16|h,a,b))
            cy.append({'SOP':'SOP','Message':name,'Msg Id':str(i%8),'Data':payload.hex(),'Start Time':str((stamp-1)*1000),'End Time':str(stamp*1000)})
        if mode:
            for kind,delta,value in [(2,delay-1,mode),(3,delay,0),(4,delay+20,0),(5,delay+21,0),(6,delay+140,7 if mode==2 else 2)]:
                events.append((at+100+delta,kind,ident,value))
    events.append((40000,7,0,0));events.sort()
    h=[MAGIC,1,MAGIC^0xffffffff,0xfffffffe,len(events),mode,100,40000,100,40,20,dropped,8,8 if mode else 0,0,0]
    b=bytearray(b'\xff'*(6*8192));struct.pack_into('<16I',b,0,*h)
    for i,r in enumerate(events):struct.pack_into('<4I',b,64+i*16,*r)
    used=64+len(events)*16;crc=0x811c9dc5
    for byte in b[:60]+b[64:used]:crc=((crc^byte)*0x01000193)&0xffffffff
    struct.pack_into('<I',b,60,crc);struct.pack_into('<I',b,8188,COMMIT)
    return b,cy

class TraceTests(unittest.TestCase):
    def test_all_three_modes_and_conservative_overlap(self):
        for mode in range(3):
            b,cy=fixture(mode);r=compare(decode(b),cy)
            self.assertEqual(r['errors'],[])
            self.assertEqual(r['critical_packets_matched'],24)
            self.assertEqual(r['captures_with_supported_overlap'],8 if mode else 0)

    def test_capture_after_transition_does_not_pass(self):
        b,cy=fixture(delay=800);r=compare(decode(b),cy)
        self.assertEqual(r['captures_with_supported_overlap'],0)
        self.assertEqual(r['status'],'review_required')

    def test_incomplete_and_corrupted_storage_are_rejected(self):
        b,_=fixture();b[8188]=0
        with self.assertRaisesRegex(ValueError,'Uncommitted'):decode(b)
        b,_=fixture();b[90]^=1
        with self.assertRaisesRegex(ValueError,'checksum'):decode(b)

    def test_overflow_and_missing_pd_packets_cannot_pass(self):
        b,cy=fixture(dropped=1)
        self.assertIn('Trace capacity exceeded',compare(decode(b),cy)['errors'])
        with self.assertRaisesRegex(ValueError,'sequences differ'):compare(decode(b),cy[:-1])

    def test_multiple_runs_require_separate_exports(self):
        b,_=fixture();b[8192:16384]=b[:8192]
        with self.assertRaisesRegex(ValueError,'found 2'):decode(b)

    def test_mode_mismatch_retains_report_and_cannot_pass(self):
        b,cy=fixture(mode=0)
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);snapshot=root/'CAPFLASH.BIN';capture=root/'CY4500.csv';report=root/'capture-comparison.json'
            snapshot.write_bytes(b)
            with capture.open('w',newline='',encoding='utf-8') as f:
                writer=csv.DictWriter(f,fieldnames=list(cy[0]));writer.writeheader();writer.writerows(cy)
            with patch('sys.argv',['analyze.py',str(snapshot),'--cy',str(capture),'--expected-mode','screen','--out',str(report)]),contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main(),1)
            result=json.loads(report.read_text())
            self.assertEqual(result['expected_mode'],'screen')
            self.assertEqual(result['run']['mode'],'baseline')
            self.assertEqual(result['run']['saved'],0)
            self.assertEqual(result['comparison']['critical_packets_matched'],24)
            self.assertEqual(result['comparison']['status'],'review_required')
            self.assertIn('Device trace mode is baseline',result['comparison']['errors'][0])
            self.assertTrue((root/'capture-trace.csv').is_file())

    def test_multiple_traces_select_unique_matching_mode_and_pd(self):
        b,cy=fixture(mode=0);screen,_=fixture(mode=1);b[8192:16384]=screen[:8192]
        result=build_report(b,cy,'screen')
        self.assertEqual(result['run']['sector'],'0x64000')
        self.assertEqual(result['run']['mode'],'screen')
        self.assertEqual(len(result['selection']['available_runs']),2)
        self.assertEqual(result['comparison']['status'],'valid_trace')

    def test_multiple_matching_traces_remain_ambiguous(self):
        b,cy=fixture();b[8192:16384]=b[:8192]
        result=build_report(b,cy,'screen')
        self.assertIsNone(result['run'])
        self.assertEqual(result['comparison']['status'],'review_required')
        self.assertIn('2 matching candidates',result['comparison']['errors'][0])

    def test_full_attempts_are_not_counted_as_successful_overlap(self):
        b,cy=fixture();h=list(struct.unpack_from('<16I',b))
        h[13]=4;h[14]=4;struct.pack_into('<16I',b,0,*h)
        for at in range(64,64+h[4]*16,16):
            ms,kind,ident,value=struct.unpack_from('<4I',b,at)
            if kind==6 and ident>4:struct.pack_into('<I',b,at+12,4)
        checksum=0x811c9dc5
        for byte in b[:60]+b[64:64+h[4]*16]:checksum=((checksum^byte)*0x01000193)&0xffffffff
        struct.pack_into('<I',b,60,checksum)
        baseline,_=fixture(mode=0);b[8192:16384]=baseline[:8192]
        result=build_report(b,cy,'screen')
        self.assertEqual(result['run']['sector'],'0x62000')
        self.assertEqual(result['run']['save_failures'],4)
        self.assertEqual(result['comparison']['status'],'review_required')
        self.assertEqual(result['comparison']['capture_attempts_with_supported_overlap'],8)
        self.assertEqual(result['comparison']['captures_with_supported_overlap'],4)
        self.assertEqual([c['save_status_name'] for c in result['comparison']['captures']],['saved']*4+['full']*4)

    def test_invalid_trace_becomes_a_review_report(self):
        b,cy=fixture();b[90]^=1
        result=build_report(b,cy,'screen')
        self.assertIsNone(result['run'])
        self.assertEqual(result['comparison']['status'],'review_required')
        self.assertIn('checksum',result['comparison']['errors'][0])

    def test_clear_check_requires_full_pool_and_preserves_other_data(self):
        data=bytearray(b'\xff'*(6*8192))
        self.assertTrue(assess(data)['ready_for_screen'])
        data[:3*8192]=b'\0'*(3*8192)
        result=assess(data)
        self.assertFalse(result['ready_for_screen'])
        self.assertEqual(result['blank_sectors'],3)
        self.assertEqual([s['kind'] for s in result['sectors']],['other_data_preserved']*3+['blank']*3)

    def test_clear_check_rejects_retained_traces_and_captures(self):
        data,_=fixture();result=assess(data)
        self.assertFalse(result['ready_for_screen'])
        self.assertEqual(result['test_traces'],1)
        data=bytearray(b'\xff'*(6*8192))
        struct.pack_into('<2I',data,8192-20,0x32435358,0xd9a2651e)
        self.assertFalse(assess(data)['ready_for_screen'])
        self.assertFalse(assess(b'bad data')['ready_for_screen'])

if __name__=='__main__':unittest.main()
