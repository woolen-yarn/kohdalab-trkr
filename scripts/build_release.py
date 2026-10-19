"""Build reviewable Windows/macOS release artifacts from this checkout only."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tomllib
import zipfile
from package_rust_portable import application_source, collect_rust_notices, zip_file
from build_rust_windows_installer import nsis_source, compiler

ROOT=Path(__file__).resolve().parents[1]


def run(*args: object) -> None:
    subprocess.run([str(a) for a in args],check=True)


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def make_zip(folder: Path, destination: Path) -> None:
    with zipfile.ZipFile(destination,'w',zipfile.ZIP_DEFLATED,strict_timestamps=False) as z:
        for path in sorted(folder.rglob('*')):
            if path.is_file() and not path.is_symlink():
                zip_file(z,path,Path(folder.name)/path.relative_to(folder))


def make_icon(destination: Path, scratch: Path) -> None:
    iconset=scratch/'AppIcon.iconset';iconset.mkdir()
    source=ROOT/'next/crates/trkr-desktop/assets/icon.png'
    for n in (16,32,128,256,512):
        for retina in (False,True):
            size=n*(2 if retina else 1)
            name=f'icon_{n}x{n}'+('@2x' if retina else '')+'.png'
            run('sips','-z',size,size,source,'--out',iconset/name)
    run('iconutil','-c','icns',iconset,'-o',destination)


def make_app(stage: Path, destination: Path, executable: str, version: str, icon: Path) -> None:
    contents=destination/'Contents';macos=contents/'MacOS';resources=contents/'Resources'
    macos.mkdir(parents=True);resources.mkdir()
    for path in stage.iterdir():
        if path.name in ('SHA256.json','kohdalab-trkr.ico'):continue
        target=macos/path.name if path.name in ('kohdalab-trkr','kohdalab-trkr-demo') else resources/path.name
        if path.is_dir():shutil.copytree(path,target)
        else:shutil.copy2(path,target)
    shutil.copy2(icon,resources/'AppIcon.icns')
    info=dict(CFBundleName=destination.stem,CFBundleDisplayName=destination.stem,
              CFBundleIdentifier='jp.kohdalab.trkr'+('.demo' if 'demo' in executable else ''),
              CFBundleExecutable=executable,CFBundleIconFile='AppIcon.icns',CFBundlePackageType='APPL',
              CFBundleShortVersionString=version,CFBundleVersion=version,
              LSMinimumSystemVersion='13.0',NSHighResolutionCapable=True)
    (contents/'Info.plist').write_bytes(plistlib.dumps(info))
    for name in ('kohdalab-trkr','kohdalab-trkr-demo','support/kohdalab-gpib-helper'):
        target=macos/name if '/' not in name else resources/name
        target.chmod(0o755)
    run('codesign','--force','--deep','--sign','-',destination)
    run('codesign','--verify','--deep','--strict',destination)


def build(platform: str, binaries: Path, native: Path, output: Path) -> list[Path]:
    version=tomllib.loads((ROOT/'next/Cargo.toml').read_text())['workspace']['package']['version']
    if os.environ.get('GITHUB_REF_TYPE')=='tag' and os.environ.get('GITHUB_REF_NAME')!='v'+version:
        raise ValueError('Tag must match the Rust workspace version')
    output=output.resolve();output.mkdir(parents=True,exist_ok=True)
    scratch=ROOT/'build'/('release-'+platform)
    scratch.mkdir(parents=True,exist_ok=True)
    source=scratch/'SOURCE.zip';application_source(source)
    notices=scratch/'rust-licenses'
    target='x86_64-pc-windows-msvc' if platform=='windows-x64' else 'aarch64-apple-darwin'
    collect_rust_notices(notices,target)
    ext='.exe' if platform=='windows-x64' else ''
    stage=scratch/f'kohdalab-trkr-{version}-{platform}'
    command=[sys.executable,ROOT/'scripts/package_rust_portable.py','--binary',binaries/('kohdalab-trkr'+ext),
             '--demo-binary',binaries/('kohdalab-trkr-demo'+ext),'--helper',native/('kohdalab-gpib-helper'+ext),
             '--source-zip',source,'--licenses',ROOT/'native/licenses','--rust-licenses',notices,'--output',stage]
    if ext:command+=['--visa-helper',native/'kohdalab-visa-helper.exe']
    run(*command)
    artifacts=[]
    source_artifact=output/f'kohdalab-trkr-{version}-{platform}-source.zip'
    shutil.copy2(source,source_artifact);artifacts.append(source_artifact)
    if ext:
        archive=output/(stage.name+'.zip');make_zip(stage,archive);artifacts.append(archive)
        setup=output/f'KohdaLab-TRKR-{version}-Setup.exe'
        nsis=scratch/'installer.nsi';nsis.write_text(nsis_source(stage,setup,version),encoding='utf-8')
        tool=compiler(None)
        if tool is None:raise RuntimeError('NSIS makensis is required')
        run(tool,nsis);artifacts.append(setup)
    else:
        if subprocess.check_output(['uname','-m'],text=True).strip()!='arm64':
            raise RuntimeError('macos-arm64 must be built on an arm64 host')
        bundle=scratch/'macos-apps';bundle.mkdir()
        icon=scratch/'AppIcon.icns';make_icon(icon,scratch)
        make_app(stage,bundle/'KohdaLab TRKR.app','kohdalab-trkr',version,icon)
        make_app(stage,bundle/'KohdaLab TRKR Demo.app','kohdalab-trkr-demo',version,icon)
        shutil.copytree(ROOT/'bindings/python',bundle/'notebook-client',ignore=shutil.ignore_patterns('__pycache__'))
        shutil.copytree(ROOT/'notebook',bundle/'notebook')
        shutil.copytree(stage/'config',bundle/'config')
        shutil.copy2(ROOT/'README.md',bundle/'README.md')
        archive=output/f'kohdalab-trkr-{version}-{platform}.zip'
        make_zip(bundle,archive);artifacts.append(archive)
        verification=scratch/'zip-verify';run('ditto','-x','-k',archive,verification)
        for app in (verification/bundle.name).glob('*.app'):run('codesign','--verify','--deep','--strict',app)
        (bundle/'Applications').symlink_to('/Applications',target_is_directory=True)
        dmg=output/f'KohdaLab-TRKR-{version}-{platform}.dmg'
        run('hdiutil','create','-volname','KohdaLab TRKR','-srcfolder',bundle,'-format','UDZO',dmg)
        artifacts.append(dmg)
    origin={key:os.environ.get(key) for key in ('GITHUB_REPOSITORY','GITHUB_SHA','GITHUB_RUN_ID','GITHUB_RUN_ATTEMPT','GITHUB_WORKFLOW','GITHUB_REF_NAME')}
    origin.update(version=version,platform=platform,signing='unsigned (macOS ad hoc only)',
                  files={p.name:sha(p) for p in artifacts},source_build=True)
    record=output/f'BUILD-ORIGIN-{platform}.json';record.write_text(json.dumps(origin,indent=2)+'\n');artifacts.append(record)
    checksums=output/f'SHA256SUMS-{platform}.txt'
    checksums.write_text(''.join(sha(p)+'  '+p.name+'\n' for p in artifacts));artifacts.append(checksums)
    return artifacts


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--platform',choices=['windows-x64','macos-arm64'],required=True)
    parser.add_argument('--binaries-dir',type=Path,required=True);parser.add_argument('--native-dir',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    for artifact in build(args.platform,args.binaries_dir.resolve(),args.native_dir.resolve(),args.output):print(artifact)
