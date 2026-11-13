"""Write explicit first-party Windows product metadata for signing review."""
import argparse
from pathlib import Path
import tomllib


def resource(filename: str, description: str, version: str) -> str:
    numeric = ','.join(version.split('-')[0].split('.') + ['0'])
    values = {'CompanyName': 'Kohdalab', 'FileDescription': description, 'FileVersion': version,
              'InternalName': Path(filename).stem, 'OriginalFilename': filename,
              'ProductName': 'KohdaLab TRKR', 'ProductVersion': version, 'LegalCopyright': 'Copyright KohdaLab contributors'}
    lines = '\n'.join(f'VALUE "{k}", "{v}\\0"' for k,v in values.items())
    return f'''1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904B0"
  BEGIN
{lines}
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x409, 1200
 END
END
'''

if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--filename',required=True);p.add_argument('--description',required=True);a=p.parse_args()
    v=tomllib.loads((Path(__file__).resolve().parents[1]/'Cargo.toml').read_text())['workspace']['package']['version']
    a.output.write_text(resource(a.filename,a.description,v))
