"""Fictional HTTP capture -> offline manifest -> AppContainer browser qualification."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
from qualify_rehearsal import inventory,run,test_denominator
from qualify_history_ui import RamSamples

REPO=Path(__file__).resolve().parent.parent

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine',type=Path,required=True)
    parser.add_argument('--python',type=Path,default=Path(sys.executable))
    parser.add_argument('--coordinator',action='store_true',help='Exercise policy/robots collection before offline replay')
    parser.add_argument('--discover',action='store_true',help='Discover transitive resources through render-only browser rounds')
    parser.add_argument('--privacy',action='store_true',help='Discover/refuse fictional private URLs before offline candidate entry')
    parser.add_argument('--custom-controls',action='store_true',help='Verify independent entry while custom widgets still block advancement')
    parser.add_argument('--failed-entry',action='store_true',help='Qualify failed input readback/image evidence without accepting the entry')
    args=parser.parse_args()
    if args.failed_entry and (args.discover or args.privacy or args.custom_controls):parser.error('--failed-entry selects a separate native scenario')
    args.discover=args.discover or args.privacy or args.custom_controls
    args.coordinator=args.coordinator or args.discover or args.failed_entry
    if os.name!='nt' or not args.engine.is_absolute() or not args.python.is_absolute() or not args.python.is_file():parser.error('Windows and explicit absolute engine/interpreter paths required')
    cargo=shutil.which('cargo')
    if not cargo:parser.error('Cargo is required for this development qualification')
    (REPO/'target').mkdir(exist_ok=True)
    args.engine=args.engine.resolve(strict=True);files=inventory(args.engine)
    folder=Path(tempfile.mkdtemp(prefix='capture-replay-qualification-',dir=REPO/'target'));content=folder/'content';content.mkdir()
    body=b'<!doctype html><title>Fictional placeholder</title>';(content/'index.html').write_bytes(body)
    sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    config={'schema':1,'engine_root':str(args.engine),'engine_files':files,'content_root':str(content),'content_files':{'index.html':hashlib.sha256(body).hexdigest()},'entry':'index.html','seconds':120}
    path=folder/'configuration.private.json';path.write_text(json.dumps(config),encoding='utf-8')
    sources=['Cargo.lock','crates/waypoint_applications/src/driver.rs','crates/waypoint_browser/src/captured_frame.rs','crates/waypoint_net/Cargo.toml','crates/waypoint_net/src/lib.rs','crates/waypoint_net/src/http.rs','crates/waypoint_browser/Cargo.toml','crates/waypoint_browser/src/rehearsal_capture.rs','crates/waypoint_browser/src/lib.rs','crates/waypoint_browser/src/rehearsal.rs','crates/waypoint_browser/src/rehearsal_worker.py','crates/waypoint_browser/src/rehearsal_replay.py','crates/waypoint_browser/tests/capture_replay_live.rs','tools/owned_process.py','tools/qualify_rehearsal.py','tools/qualify_history_ui.py','tools/qualify_capture_replay.py']
    if args.coordinator:sources.extend(['crates/waypoint_browser/src/capture_privacy.rs','crates/waypoint_browser/src/capture_session.rs','crates/waypoint_browser/src/capture_discovery.rs','crates/waypoint_browser/src/capture_session_qualification.rs','crates/waypoint_policy/src/lib.rs','crates/waypoint_policy/data/source_permissions.json'])
    hashes=lambda:{name:sha(REPO/name) for name in sources}
    report={'complete':False,'scope':'fictional response bytes to offline snapshot and contained browser; no public source or employer-form capture','source_sha256':hashes(),'configuration_sha256':sha(path)}
    report['coordinator']=args.coordinator
    report['discovery']=args.discover
    report['privacy']=args.privacy
    env=os.environ.copy();env.update(WAYPOINT_REHEARSAL_CONFIG=str(path),WAYPOINT_REHEARSAL_CONFIG_SHA256=report['configuration_sha256'],WAYPOINT_REHEARSAL_PYTHON=str(args.python),CARGO_BUILD_JOBS='2')
    memory=RamSamples();memory.thread.start()
    try:
        case='discovery_archives_transitive_resources_before_candidate_entry' if args.discover else 'coordinator_archive_drives_contained_form'
        if args.privacy:case='private_resources_are_not_fetched_before_offline_candidate_entry'
        if args.custom_controls:case='independent_fields_preserve_custom_review_and_hard_barriers'
        if args.failed_entry:case='failed_entry_retains_bound_readback_without_approving_it'
        selection=['--lib','capture_session::qualification::'+case] if args.coordinator else ['--test','capture_replay_live']
        log=folder/'native.private.log';report['run']=run([cargo,'test','-p','waypoint_browser',*selection,'--','--ignored','--nocapture','--test-threads=1'],env,log,180)
        if report['run']['exit_code']:raise RuntimeError('Capture bridge failed; inspect '+str(log))
        report['tests']=test_denominator(log.read_text(encoding='utf-8'),'rust',1)
        native=folder/'http-captured.private.json'
        if json.loads(native.read_text())['complete'] is not True:raise RuntimeError('Missing native completion')
        report['native_sha256']=sha(native)
        (folder/'screenshot.png').write_bytes(base64.b64decode((folder/'http-captured-screenshot.base64').read_bytes(),validate=True))
        report['screenshot_sha256']=sha(folder/'screenshot.png')
        if args.failed_entry:
            failure=json.loads((folder/'failed-entry.private.json').read_bytes())
            if failure.get('status')!='entry_unverified' or failure.get('entry_verified') is not False:raise RuntimeError('Failed entry was mislabeled')
            (folder/'failed-entry.png').write_bytes(base64.b64decode((folder/'failed-entry-screenshot.base64').read_bytes(),validate=True))
            report['failed_entry_sha256']=sha(folder/'failed-entry.private.json')
            report['failed_entry_image_sha256']=sha(folder/'failed-entry.png')
        if hashes()!=report['source_sha256'] or inventory(args.engine)!=files:raise RuntimeError('Source or engine changed during capture qualification')
        report['complete']=True
    finally:
        report['memory']=memory.finish();(folder/'result.private.json').write_text(json.dumps(report,indent=2),encoding='utf-8');print('Private capture bridge evidence:',folder)

if __name__=='__main__':main()
