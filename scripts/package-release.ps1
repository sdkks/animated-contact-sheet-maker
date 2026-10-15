# Called by package-release.sh with native Windows paths in the environment.
$ErrorActionPreference = 'Stop'
$files = Get-ChildItem -LiteralPath $env:PACKAGE_STAGE -File
Compress-Archive -LiteralPath $files.FullName -DestinationPath $env:PACKAGE_ARCHIVE
(Get-FileHash -LiteralPath $env:PACKAGE_ARCHIVE -Algorithm SHA256).Hash.ToLowerInvariant()
