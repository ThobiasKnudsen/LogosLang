# Provenance

Evidence that this repository existed, in this exact state, on a given date, provable
without trusting GitHub, and without trusting Thobias either.

Git's own timestamps are written by whoever makes the commit and can be set to any
value, so they prove nothing on their own. That is the objection anybody would raise,
and the answer is a countersignature from somebody with no stake in the outcome.

## What is here

| File | What it is |
|---|---|
| `manifest-YYYY-MM-DD.txt` | The commit hash, the tree hash, the commit count, the release tags, and the SHA-256 of the governing documents on that date. A commit hash covers the whole tree and every commit behind it, so fixing that one line fixes the entire history. |
| `manifest-YYYY-MM-DD.freetsa.tsr` | An RFC 3161 timestamp token over the manifest, issued by FreeTSA (Germany). |
| `manifest-YYYY-MM-DD.digicert.tsr` | The same manifest, countersigned independently by DigiCert. Two authorities in two jurisdictions so no single one has to be believed. |
| `freetsa-cacert.pem`, `freetsa-tsa.crt` | FreeTSA's certificates, kept here so the token stays verifiable if their site goes away. DigiCert's roots ship with every operating system. |

## Verifying

Anyone can check these, offline, with nothing but OpenSSL:

```sh
openssl ts -verify -in manifest-2026-09-10.freetsa.tsr \
    -data manifest-2026-09-10.txt \
    -CAfile freetsa-cacert.pem -untrusted freetsa-tsa.crt

openssl ts -verify -in manifest-2026-09-10.digicert.tsr \
    -data manifest-2026-09-10.txt \
    -CAfile /etc/ssl/certs/ca-certificates.crt
```

Both must print `Verification: OK`. Read the issued time with:

```sh
openssl ts -reply -in manifest-2026-09-10.digicert.tsr -text | grep 'Time stamp'
```

Then confirm the manifest describes this repository:

```sh
git cat-file -t $(grep -oP '(?<=commit  )[0-9a-f]{40}' manifest-2026-09-10.txt)
```

A token verifies only against the byte-exact manifest. Editing the manifest breaks it,
which is the point.

## The other copies

A timestamp proves *when*. These prove the code itself survives somewhere other than one
company's servers:

**Software Heritage**, the UNESCO-backed permanent source-code archive, holds full
snapshots. First taken 10 September 2026, refreshed at least monthly by
`.github/workflows/archive.yml`. A snapshot identifier names the exact set of branches
and tags archived, and resolves for good:

| Repository | First snapshot |
|---|---|
| LogosLang | `swh:1:snp:6d07b2b9e7a5721fcaf6d6f5206f7c838ce574a0` |
| LogosMath | `swh:1:snp:ac6f38608af5247fa35a2f0147b993dee71302f8` |
| LogosLangWebsite | `swh:1:snp:73310944762489f4a9a2c44eedb3756ad22e989b` |

Resolve one at `https://archive.softwareheritage.org/<swhid>`.

**GH Archive** publishes the public GitHub event stream as hourly dumps, independent of
GitHub, and mirrors them into a public BigQuery dataset. It records when each push
actually happened, which no later rewrite of git history can alter. The earliest public
use of the name *LogosLang* is a push recorded there at `2026-01-05T17:02:38Z`, in
`https://data.gharchive.org/2026-01-05-17.json.gz`.

**The Wayback Machine** holds captures from 10 September 2026 of
<https://web.archive.org/web/20260910183038/https://logoslang.dev/> and
<https://web.archive.org/web/20260910183056/https://github.com/ThobiasKnudsen/LogosLang>.

## Adding a new one

Rebuild the manifest, get it countersigned, verify before committing:

```sh
cd provenance
D=$(date +%F)
{
  echo "commit  $(git rev-parse HEAD)"
  echo "tree    $(git rev-parse HEAD^{tree})"
} > /tmp/head.txt   # or copy an existing manifest and update it
openssl ts -query -data manifest-$D.txt -sha256 -cert -out request.tsq
curl -H "Content-Type: application/timestamp-query" --data-binary @request.tsq \
    https://freetsa.org/tsr -o manifest-$D.freetsa.tsr
curl -H "Content-Type: application/timestamp-query" --data-binary @request.tsq \
    http://timestamp.digicert.com -o manifest-$D.digicert.tsr
```

Keep the old ones. Each is a separate dated claim, and the earliest is the one that
matters.
