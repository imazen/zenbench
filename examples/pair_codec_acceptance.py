#!/usr/bin/env python3
"""Small real-codec adapter acceptance; no codec performance claim.

Run prepare through the host resource wrapper. Programs are prebuilt; this
script never builds, checks out source, or changes their environment globally.
The plans exercise the current rav1d-safe profile_ivf and zenav1-svt perf_encode /
perf_c_encode protocols from https://github.com/imazen/zenbench/issues/29.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def decoder_check(left, right):
    rows=[]
    for directory in (left,right):
        stderr=(directory/'stderr.txt').read_text()
        if re.search(r'(decode|flush) error',stderr,re.I):
            raise ValueError('decoder reported an error')
        records=[line.split('\t') for line in (directory/'stdout.txt').read_text().splitlines()
                 if line.startswith('RESULT\t')]
        if len(records)!=3: raise ValueError('expected three retained passes')
        counts=[(int(row[3]),int(row[4])) for row in records]
        if len(set(counts))!=1 or min(counts[0])<=0:
            raise ValueError('missing/inconsistent iteration or frame count')
        rows.append(counts)
    if rows[0]!=rows[1]: raise ValueError('baseline/candidate work counts differ')


def encoder_check(left, right):
    if (left/'result.obu').read_bytes()!=(right/'result.obu').read_bytes():
        raise ValueError('encoded bytes differ')
    if not (left/'result.obu').stat().st_size: raise ValueError('empty encoded output')


def prepare(args):
    root=args.output.resolve()
    root.mkdir(parents=True,exist_ok=False)
    profile=args.profile.resolve(); encode=args.encode.resolve(); c_encode=args.c_encode.resolve()
    ivf=args.input.resolve(); checker=Path(__file__).resolve()
    def arm(name,program,argv,revision,env=None):
        return dict(id=name,program=str(program),args=argv,revision=revision,timeout_seconds=30,env=env or {})
    def common(baseline,candidates,cases,metric,check,warmup):
        return dict(schema=1,baseline=baseline,candidates=candidates,cases=cases,metric=metric,
                    check=check,warmup=warmup,blocks=4,seed=29,deadline_seconds=300)
    def check(kind):
        return dict(kind='command',program=sys.executable,args=[str(checker),kind,'${baseline}','${candidate}'],timeout_seconds=10)
    env=dict(RAV1D_THREADS='${threads}',RAV1D_REPS='3',RAV1D_FRAME_DELAY='1',
             RAV1D_LEVEL='native',RAV1D_INLOOP='all',RAV1D_PPROF=None,RAV1D_ABLATE=None)
    decoder=common(arm('before',profile,['${input}','1'],args.profile_rev,env),
        [arm('after',profile,['${input}','1'],args.profile_rev,dict(env,RAV1D_LABEL='candidate'))],
        [dict(id=f'threads-{n}',fields=dict(threads=str(n)),inputs=dict(input=str(ivf),checker=str(checker))) for n in (1,4)],
        dict(extract=dict(kind='tsv',prefix='RESULT\t',column=5),unit='ms',expected=3),
        check('check-decoder'),'profile_ivf performs one untimed warmup and emits three timed per-frame passes')
    # Generate and freeze the C input before randomizing any timing arms.
    setup=subprocess.run([str(encode),'gradient','64','64','40','10',str(root/'input'),'1'],capture_output=True,check=True)
    (root/'setup.stdout.txt').write_bytes(setup.stdout); (root/'setup.stderr.txt').write_bytes(setup.stderr)
    raw=root/'input.yuv'
    if not raw.is_file() or not raw.stat().st_size: raise ValueError('encoder did not emit input.yuv')
    encoder=common(arm('rust',encode,['gradient','64','64','40','10','${output}/result','1'],args.encode_rev),
        [arm('c',c_encode,['64','64','40','10','${input}','${output}/result.obu','1'],args.c_rev)],
        [dict(id='gradient-64-p10-q40',inputs=dict(input=str(raw),checker=str(checker)))],
        dict(extract=dict(kind='regex',pattern=r'ENCODE_NS=(\d+)',capture=1),unit='ns',expected=1),
        check('check-encoder'),'each driver performs one excluded warmup encode')
    for name,plan in [('decoder',decoder),('encoder',encoder)]:
        (root/f'{name}.json').write_text(json.dumps(plan,indent=2)+'\n')
    (root/'identity.json').write_text(json.dumps(dict(profile=digest(profile),encode=digest(encode),
        c_encode=digest(c_encode),ivf=digest(ivf),yuv=digest(raw),checker=digest(checker)),indent=2)+'\n')
    print(root)


def audit(path):
    import gzip
    import math
    import statistics
    with gzip.open(path,'rt') as source: packet=json.load(source)
    results=[]
    for ci,case in enumerate(packet['plan']['cases']):
        rows=[i for i in packet['invocations'] if i['case']==ci]
        if any(i['status']!='completed' for i in rows): raise ValueError('failed invocation')
        narms=len(packet['plan']['candidates'])+1+int(packet['plan']['aa'])
        if len(rows)!=packet['plan']['blocks']*narms: raise ValueError('missing invocation')
        for arm in range(1,narms):
            ratios=[]
            for block in range(packet['plan']['blocks']):
                group=[i for i in rows if i['block']==block]
                if sorted(i['arm'] for i in group)!=list(range(narms)): raise ValueError('invalid block')
                values={i['arm']:statistics.mean(i['values_ns']) for i in group}
                ratios.append(values[arm]/values[0])
            estimate=100*math.expm1(statistics.mean(math.log(x) for x in ratios))
            summary=packet['summaries'][ci*(narms-1)+arm-1]
            if not math.isclose(estimate,summary['latency_change_pct'],abs_tol=1e-8):
                raise ValueError('independent estimate differs')
            results.append(dict(case=case['id'],arm=summary['arm'],blocks=len(ratios),
                                estimate_pct=estimate,ratio_median=statistics.median(ratios)))
    print(json.dumps(dict(packet_sha256=digest(path),complete=packet['complete'],
        elapsed_seconds=packet['elapsed_seconds'],invocations=len(packet['invocations']),results=results),indent=2))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    p=sub.add_parser('prepare')
    for key in ('profile','encode','c-encode','input','output'): p.add_argument('--'+key,type=Path,required=True)
    for key in ('profile-rev','encode-rev','c-rev'): p.add_argument('--'+key,required=True)
    for name in ('check-decoder','check-encoder'):
        p=sub.add_parser(name); p.add_argument('left',type=Path); p.add_argument('right',type=Path)
    p=sub.add_parser('audit'); p.add_argument('packet',type=Path)
    args=parser.parse_args()
    if args.command=='prepare': prepare(args)
    elif args.command=='audit': audit(args.packet)
    elif args.command=='check-decoder': decoder_check(args.left,args.right)
    else: encoder_check(args.left,args.right)

if __name__=='__main__': main()
