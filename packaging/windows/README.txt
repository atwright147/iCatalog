iCatalog on Windows
===================

Extract this ZIP to a permanent folder. Run icatalog-ui.exe to open the default
catalogue, or pass a catalogue path to it from a terminal.

To offer iCatalog for .icatalog files, run this command from the extracted
folder in PowerShell:

    powershell -NoProfile -File .\associate.ps1 Register

Then choose iCatalog for .icatalog in Settings > Apps > Default apps, or use
Open with in File Explorer. Windows keeps the default app choice under your
control. Existing .sqlite catalogues still open when passed to icatalog-ui.exe;
they are not registered as a file type.

Before moving or removing the extracted folder, run:

    powershell -NoProfile -File .\associate.ps1 Unregister

If Windows blocks the downloaded script, review it first, then run
`Unblock-File .\associate.ps1` in PowerShell and retry.

The helper changes only this user's iCatalog registration. It does not require
administrator rights or change the current default app.
