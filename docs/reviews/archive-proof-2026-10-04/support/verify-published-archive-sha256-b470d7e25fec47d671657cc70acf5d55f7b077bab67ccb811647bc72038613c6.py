import sys,pathlib,json,hashlib,subprocess,tarfile,re,datetime,os
p=pathlib.Path(sys.argv[1]); log=[]
def run(*args):
 c=['rtk','proxy',*map(str,args)]; r=subprocess.run(c,capture_output=True,check=True); log.append({'argv':c,'returncode':r.returncode,'stdout':r.stdout.decode(errors='replace') if len(r.stdout)<10000 else {'bytes':len(r.stdout),'sha256':hashlib.sha256(r.stdout).hexdigest()},'stderr':r.stderr.decode(errors='replace')});return r.stdout.decode().strip()
def sha(path):return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
def load(n):return json.loads((p/n).read_text())
r=load('release-api.json');latest=load('latest-api.json');tag=load('tag-api.json')
assert r['id']==402793692 and r['immutable'] is True and r['draft'] is False and r['published_at']=='2026-10-04T02:44:00Z'
assert r['tag_name']=='archive-owned-source-2026-10-04' and r['target_commitish']=='47815c83b9eeadbaf84b741918fffa7ea550da89'
assert latest['id']!=r['id']; assert tag['object']['type']=='commit' and tag['object']['sha']==r['target_commitish']
expected={'SHA256SUMS','archive-manifest.json','archive-plan.md','external-bundle-results.json','external-bundles.tar','gitlink-inventory.json','nested-gitlink-inventory.json','six-owned-source-refs.bundle'}
assert len(r['assets'])==8 and {a['name'] for a in r['assets']}==expected
assets=[]
for a in r['assets']:
 f=p/a['name']; h=sha(f);assert f.stat().st_size==a['size'] and a['digest']=='sha256:'+h and a['state']=='uploaded';assets.append({'id':a['id'],'name':a['name'],'bytes':a['size'],'sha256':h,'api_digest_match':True})
assert sha(p/'SHA256SUMS')=='a5772df6ce36f61754cb710c29c29dd77fb2a98d916482816c4d3960903cd391'
checks={}
for line in (p/'SHA256SUMS').read_text().splitlines():
 h,n=line.split('  ',1);assert re.fullmatch('[0-9a-f]{64}',h) and n not in checks and not pathlib.PurePosixPath(n).is_absolute() and '..' not in pathlib.PurePosixPath(n).parts;checks[n]=h
assert len(checks)==31
# Outer checksum must pass before any tar listing or extraction.
assert sha(p/'external-bundles.tar')==checks['external-bundles.tar']=='7ffc252811bfc2f88af2e69c90b3a2ec175867e12e6efa68f004f84e068ef1ae'
e=load('external-bundle-results.json');m=load('archive-manifest.json');g=load('gitlink-inventory.json')
expected_members=e['portable_container']['members'];assert len(set(expected_members))==24
with tarfile.open(p/'external-bundles.tar','r:') as t:
 members=t.getmembers();names=[x.name for x in members];assert len(members)==24 and len(set(names))==24 and set(names)==set(expected_members)
 for x in members:
  parts=pathlib.PurePosixPath(x.name).parts;assert x.isfile() and not x.issym() and not x.islnk() and len(parts)==2 and parts[0]=='external-bundles' and re.fullmatch('[a-zA-Z0-9_.-]+\\.bundle',parts[1]) and not pathlib.PurePosixPath(x.name).is_absolute() and '..' not in parts and x.name=='/'.join(parts)
 for x in members:
  dest=p/x.name;dest.parent.mkdir(exist_ok=True);dest.write_bytes(t.extractfile(x).read())
for n,h in checks.items():assert sha(p/n)==h
assert set(checks)==(expected-{'SHA256SUMS'})|set(expected_members)
restores=p/'fresh-bare-restores';restores.mkdir()
def restore(bundle,dest):
 run('git','init','--bare',dest);v=run('git','-C',dest,'bundle','verify',bundle);run('git','-C',dest,'fetch',bundle,'+refs/*:refs/*');run('git','-C',dest,'fsck','--full','--strict','--no-reflogs');assert not (dest/'objects/info/alternates').exists();assert run('git','-C',dest,'rev-parse','--is-shallow-repository')=='false';return v
main=restores/'main.git';restore(p/'six-owned-source-refs.bundle',main)
refs=dict(line.split(' ',1)[::-1] for line in run('git','-C',main,'for-each-ref','--format=%(objectname) %(refname)').splitlines())
assert refs=={x['restored_ref']:x['commit'] for x in m['main_bundle']['refs']}
main_results=[]
for x in m['main_bundle']['refs']:
 tree=run('git','-C',main,'rev-parse',x['commit']+'^{tree}');count=int(run('git','-C',main,'rev-list','--count',x['commit']));assert tree==x['tree'] and count==x['reachable_commit_count'];main_results.append({'name':x['name'],'ref':x['restored_ref'],'commit':x['commit'],'tree':tree,'reachable_commit_count':count})
external=[];all_oids=set();repo_paths={}
for x in e['results']:
 dest=restores/(x['slug']+'.git');restore(p/x['bundle_member'],dest); assert sha(p/x['bundle_member'])==x['bundle_sha256'] and (p/x['bundle_member']).stat().st_size==x['bundle_bytes'];repo_paths[x['bundle_member']]=dest
 inv=next(y for y in g['external_repositories'] if y['bundle_member']==x['bundle_member']);oids=inv['gitlink_commit_oids'];assert len(oids)==inv['distinct_commit_count']
 for oid in oids:assert run('git','-C',dest,'cat-file','-t',oid)=='commit';run('git','-C',dest,'rev-list','--objects','--missing=error',oid);all_oids.add(oid)
 external.append({'member':x['bundle_member'],'bytes':x['bundle_bytes'],'sha256':x['bundle_sha256'],'required_commit_oids':oids,'bundle_verify':True,'fsck_full_strict':True,'no_alternates':True,'not_shallow':True})
for x in g['historical_nested_dependencies']:
 dest=repo_paths[x['included_in_bundle']];assert run('git','-C',dest,'cat-file','-t',x['commit'])=='commit';run('git','-C',dest,'rev-list','--objects','--missing=error',x['commit']);all_oids.add(x['commit'])
assert len(external)==24 and len(all_oids)==183
runtime_path=pathlib.Path('/local/home')
records=[json.loads(l) for l in runtime_path.read_text().splitlines()];ctx=[x for x in records if x['type']=='turn_context'][-1];task=next(x for x in records if 'NEW_TASK' in str(x) and 'Task name: /root/archive_postpublication_review_v3' in str(x));assert ctx['payload']['model']=='gpt-6.1-sol' and ctx['payload']['effort']=='medium'
proof={'schema_version':1,'status':'GO_ARCHIVE_RECOVERABILITY_ONLY','verified_at_utc':datetime.datetime.now(datetime.UTC).isoformat(),'fresh_download_directory':str(p),'fresh_downloads_only':True,'no_existing_restore_or_alternates_used':True,'runtime':{'model':ctx['payload']['model'],'reasoning_effort':ctx['payload']['effort'],'rollout':str(runtime_path),'turn_context_timestamp':ctx['timestamp'],'turn_id':ctx['payload']['turn_id'],'new_task_timestamp':task['timestamp'],'new_task_ordinal':task['ordinal']},'release':{'id':r['id'],'url':r['html_url'],'immutable':r['immutable'],'published_at':r['published_at'],'tag':r['tag_name'],'target_commit':tag['object']['sha'],'latest_release_id':latest['id'],'latest_release_tag':latest['tag_name'],'archive_is_latest':False},'assets':assets,'checksum_fingerprint_sha256':sha(p/'SHA256SUMS'),'all_checksum_entries_verified':31,'outer_tar_checksum_before_listing_and_extraction':True,'tar':{'bytes':(p/'external-bundles.tar').stat().st_size,'sha256':sha(p/'external-bundles.tar'),'member_count':24,'members':names,'exact_shape_validated':True,'regular_relative_only':True,'no_traversal_links_or_duplicates':True},'main_bundle':{'bytes':(p/'six-owned-source-refs.bundle').stat().st_size,'sha256':sha(p/'six-owned-source-refs.bundle'),'bundle_verify':True,'fsck_full_strict':True,'exact_ref_count':6,'no_alternates':True,'not_shallow':True,'refs':main_results},'external_bundles':external,'distinct_required_external_gitlink_commits_verified':len(all_oids),'retirement_authorized':False,'action_sha_retention_proven':False,'source_ref_or_tag_deletions_performed':False,'remote_mutations_performed':False,'limits':['Archive recoverability proof does not authorize retirement or prove GitHub Actions SHA retention.']}
(p/'verification-command-log.json').write_text(json.dumps(log,indent=2)+'\n');(p/'post-publication-proof.json').write_text(json.dumps(proof,indent=2)+'\n');h=sha(p/'post-publication-proof.json');(p/'post-publication-proof.json.sha256').write_text(h+'  post-publication-proof.json\n');print(json.dumps({'status':proof['status'],'proof':str(p/'post-publication-proof.json'),'sha256':h,'latest_release_tag':latest['tag_name'],'checksums':31,'main_refs':6,'external_bundles':24,'gitlink_oids':183}))
