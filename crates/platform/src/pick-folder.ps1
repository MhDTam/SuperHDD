$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false)
Add-Type -AssemblyName System.Windows.Forms
$dialog=New-Object Windows.Forms.FolderBrowserDialog
$dialog.Description='Super HDD: game or program folder (includes subfolders)'
$dialog.ShowNewFolderButton=$false
if([IO.Directory]::Exists('D:\GAMES')){$dialog.SelectedPath='D:\GAMES'}
try{if($dialog.ShowDialog() -eq [Windows.Forms.DialogResult]::OK){[Console]::Write($dialog.SelectedPath)}}finally{$dialog.Dispose()}
