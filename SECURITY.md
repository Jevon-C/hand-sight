# Security and data handling

Hand Sight is a research prototype that can handle videos of people's hands. Treat all uploaded
videos and results as personal data.

- The server listens on `127.0.0.1` only. Making it reachable from other computers needs an explicit
  setting (`allow_remote`) and, before any real use, sign-in, HTTPS and a storage and retention plan.
  It has none of these built in.
- Videos, results and the index are stored in a local data folder (`data/` by default) and are never
  sent anywhere by the software. The data folder is excluded from version control.
- Plug-ins run as separate processes with a time limit and an output size limit, and their output
  is validated before it is stored. They are not sandboxed beyond that: only install plug-ins you trust.
- Uploads are checked by content, not by file name, and file names are shown only after cleaning.

## Reporting a problem

Please report a vulnerability privately by email to the repository owner, or through GitHub's
private vulnerability reporting, rather than in a public issue.
