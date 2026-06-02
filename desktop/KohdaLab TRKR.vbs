Option Explicit

Dim shell, fso, projectRoot, pythonwPath, uvPath, command

Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")

projectRoot = fso.GetParentFolderName(fso.GetParentFolderName(WScript.ScriptFullName))
pythonwPath = projectRoot & "\.venv\Scripts\pythonw.exe"
uvPath = shell.ExpandEnvironmentStrings("%USERPROFILE%") & "\.local\bin\uv.exe"

If fso.FileExists(pythonwPath) Then
  command = "cmd.exe /c cd /d """ & projectRoot & """ && set PYTHONPATH=src && """ & pythonwPath & """ -m kohdalab.apps.trkr_gui > gui_launcher.log 2>&1"
Else
  command = "cmd.exe /c cd /d """ & projectRoot & """ && """ & uvPath & """ run --extra gui python -m kohdalab.apps.trkr_gui > gui_launcher.log 2>&1"
End If

shell.Run command, 0, False
