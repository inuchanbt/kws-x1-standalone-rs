"""Use the reviewed ASD/CY4500 sweep with capture-test-specific operator prompts."""
import argparse
import importlib.util
import json
import secrets
import sys
from pathlib import Path

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode',required=True,choices=['baseline','screen','tall'])
    parser.add_argument('--port',default='COM3')
    parser.add_argument('--plan',action='store_true')
    args=parser.parse_args()
    path=ROOT/'tools/logger-sweep/kws_avs_sweep.py'
    spec=importlib.util.spec_from_file_location('capture_test_sweep',path)
    sweep=importlib.util.module_from_spec(spec);sys.modules[spec.name]=sweep
    source=path.read_text(encoding='utf-8')
    label='BASE' if args.mode=='baseline' else args.mode.upper()
    prompts={
        '[READY] Start KWS logger, then press Enter to sweep. LOAD remains ON.':f'[READY] After connecting the measurement wiring, select {label} on KWS (BACK+OK cycles modes; power loss resets to BASE). Hold BACK+LEFT+RIGHT for 1 second, confirm {label} RUNNING, then press Enter.',
        '[DONE] Stop KWS logger; wait for LOGGER STOPPED. Then press Enter to turn LOAD OFF and save CY4500.':'[DONE] On KWS hold BACK+LEFT+RIGHT for 1 second; wait for TEST SAVED, then press Enter to turn LOAD OFF and save CY4500.',
    }
    for old,new in prompts.items():
        if source.count(old)!=1:raise RuntimeError('Sweep prompt changed; review the adapter before use')
        source=source.replace(old,new)
    source=source.replace('DONE/Enter after LOGGER STOPPED','DONE/Enter after TEST SAVED')
    exec(compile(source,str(path),'exec'),sweep.__dict__)
    holds=[3.0+0.2*secrets.randbelow(8) for _ in range(9)]
    class TimedRoute(tuple):
        def __iter__(self):
            for i,target in enumerate(super().__iter__()):
                sweep.HOLD=holds[i]
                yield target
    sweep.TARGETS=TimedRoute(sweep.TARGETS)
    directory=ROOT/'CY4500_logs'/('capture-autotest-'+args.mode)
    before=set(directory.glob('kws_avs_*')) if directory.exists() else set()
    sys.argv=[str(path),'--port',args.port,'--panel-object','9','--output-dir',str(directory)]
    if args.plan:sys.argv.append('--plan')
    completed=False
    try:
        result=sweep.main()
        completed=result is None or result==0
        return result
    finally:
        for run in set(directory.glob('kws_avs_*'))-before:
            (run/'capture-test-plan.json').write_text(json.dumps({'mode':args.mode,'hold_seconds':holds,'targets':list(tuple.__iter__(sweep.TARGETS)),'reason':'Per-run dwell timing makes accidental cross-run clock fits detectable; all holds remain at least three seconds.'},indent=2)+'\n',encoding='utf-8')
            if completed and (run/'CY4500.csv').is_file():
                print('[EXPORT NEXT] Reconnect KWS to PC holding LEFT+RIGHT, then run this command (adjust F: if needed):')
                print(f'.\\tools\\capture-autotest\\collect.ps1 -RunDirectory "{run.resolve()}" -Drive F:\\')

if __name__=='__main__':raise SystemExit(main())
