# Security policy

## Reporting a vulnerability

Please report security issues privately through
[GitHub's private vulnerability reporting](https://github.com/Nokama0/ShowTrack/security/advisories/new)
rather than opening a public issue. I will respond as soon as I can.

## Supported versions

Only the latest release receives fixes. Update from the
[releases page](https://github.com/Nokama0/ShowTrack/releases/latest) or via
**Settings > Maintenance > Check for updates**.

## Scope and design notes

ShowTrack is local-first: your library never leaves your devices, and the only
network dependency is TheMovieDB for metadata. A few areas are security-relevant
by design:

- **Phone sync** pairs over your local network. A 6-digit code (single use, valid
  5 minutes) mints a 32-byte bearer token, compared in constant time. There is no
  TLS by design; it relies on your Wi-Fi link encryption and never leaves the LAN.
  Reports about the pairing flow or token handling are welcome.
- **Your TMDB API key** is stored in the local database and is included in backup
  JSON files. Treat exported backups as secret.
- Release binaries are unsigned, so the first launch triggers a Windows SmartScreen
  warning. Verify downloads come from the official releases page.
