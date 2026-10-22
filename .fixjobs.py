import json,collections,subprocess,sys
def run():
    out=subprocess.run("CARGO_BUILD_JOBS=2 nice -n 10 mise exec -- cargo check --locked --workspace --all-targets --message-format=json 2>/dev/null",shell=True,capture_output=True,text=True).stdout
    sites=collections.defaultdict(set); errs=[]
    for line in out.splitlines():
        try: m=json.loads(line)
        except: continue
        if m.get('reason')!='compiler-message': continue
        msg=m['message']
        if msg['level']!='error': continue
        if 'missing field' in msg['message'] and 'services' in msg['message']:
            sp=[s for s in msg['spans'] if s['is_primary']][0]
            sites[sp['file_name']].add(sp['line_start'])
        else: errs.append(msg['rendered'][:300])
    return sites,errs
for _ in range(6):
    sites,errs=run()
    if not sites:
        print("remaining non-services errors:",errs[:3]); break
    for f,lines in sites.items():
        src=open(f).read().split('\n')
        for ln in sorted(lines,reverse=True):
            line=src[ln-1]
            indent=len(line)-len(line.lstrip())
            if line.rstrip().endswith('{'):
                src.insert(ln,' '*(indent+4)+'services: Vec::new(),')
            else:
                print("manual:",f,ln,line)
        open(f,'w').write('\n'.join(src))
    print("fixed",sum(len(v) for v in sites.values()))
