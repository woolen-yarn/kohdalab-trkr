Option Explicit

Dim shell, fso, projectDir, env, oldPythonPath, srcPath, command

Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")

projectDir = fso.GetParentFolderName(WScript.ScriptFullName)
If Not fso.FileExists(projectDir & "\src\kohdalab\apps\trkr_gui.py") Then
    projectDir = "C:\pythonKernel\kohdalab-trkr"
End If

If Not fso.FileExists(projectDir & "\src\kohdalab\apps\trkr_gui.py") Then
    MsgBox "Could not find src\kohdalab\apps\trkr_gui.py." & vbCrLf & projectDir, vbCritical, "Kohda Lab TRKR"
    WScript.Quit 1
End If

shell.CurrentDirectory = projectDir

Set env = shell.Environment("PROCESS")
oldPythonPath = env("PYTHONPATH")
srcPath = projectDir & "\src"
If Len(oldPythonPath) > 0 Then
    env("PYTHONPATH") = srcPath & ";" & projectDir & ";" & oldPythonPath
Else
    env("PYTHONPATH") = srcPath & ";" & projectDir
End If

command = "cmd.exe /c ""uv run pythonw -m kohdalab.apps.trkr_gui"""
shell.Run command, 0, False
