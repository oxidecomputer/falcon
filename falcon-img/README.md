# falcon-img

A caching image server for falcon. It proxies everything falcon downloads
(base images, OVMF firmware and propolis-server builds) and keeps a local
copy, so each asset only crosses the WAN once per site.

## Quick start

```shell
cargo build --release -p falcon-img
pfexec ./target/release/falcon-img serve      # or install it as a service, below
```

Then make the name `falcon-img` resolve to that host for the machines that run
falcon, e.g. an `/etc/hosts` entry or a DNS record. That's all: falcon
discovers it on its own.

## How falcon finds it

During `launch`, `preflight` and `hyperstart`, the first time falcon needs to
download something it:

1. Resolves `falcon-img`. If the name doesn't resolve, falcon downloads from
   the upstream sources as it always has.
2. Asks `http://falcon-img:7780/v1/discover` whether it's a falcon image
   server. If the name resolves but this check fails, falcon stops with an
   error. A broken image server is never silently bypassed.
3. Downloads everything through the image server. If the image server can't
   provide something, that's an error too.

These options control discovery for `launch`, `preflight` and `hyperstart`:

| Flag                         | Environment                 | Effect                                                    |
|------------------------------|-----------------------------|-----------------------------------------------------------|
| `--img-server <host[:port]>` | `FALCON_IMG_SERVER`         | Use this server instead. If it can't be reached, that's an error. |
| `--no-img-discovery`         | `FALCON_NO_IMG_DISCOVERY=1` | Skip the image server; download directly from upstream.    |

Programs that drive `Runner` directly use `Runner::set_img_server` and
`Runner::set_img_discovery`. Both default to the environment variables above.

## Air-gapped use

Place assets in the cache directory, using the same names they have upstream:

```shell
pfexec mv helios-2.5_0.raw.xz OVMF_CODE.fd OVMF_CODE.fd.sha256.txt $(falcon-img dir)
pfexec mkdir -p $(falcon-img dir)/propolis/<rev>
pfexec mv propolis-server propolis-server.sha256.txt $(falcon-img dir)/propolis/<rev>/
```

`<rev>` is the propolis commit falcon is pinned to (the `propolis-client` rev
in the workspace `Cargo.toml`). Then run the server with `--offline` (or
`FALCON_IMG_OFFLINE=true`), so it never tries to reach upstream.

Files are served as soon as they exist. When copying across filesystems, copy
to a temporary name in the cache directory first and then `mv` it into place,
so a partially copied image is never served.

## Caching behavior

- Base images (`*.raw.xz`) and propolis builds are treated as immutable. Once
  cached, they're always served from the cache.
- Other assets (`OVMF_CODE.fd` and its digest) can change upstream. The server
  revalidates them with an `If-None-Match` request on every fetch, and falls
  back to the cached copy when upstream is unreachable.
- On a cache miss, the download is streamed to the client while it's written
  to the cache. The file only appears in the cache once it's complete.

## Running as a service

```shell
pfexec ./falcon-img/pkg/install.sh target/release/falcon-img   # illumos (SMF)
sudo   ./falcon-img/pkg/install.sh target/release/falcon-img   # Linux (systemd)
```

This installs the binary to `/opt/falcon-img/bin` and the cache to
`/var/falcon-img`.

- **SMF:** set the `config/listen`, `config/dir` and `config/offline`
  properties on `svc:/oxide/falcon-img:default`, e.g.
  `svccfg -s falcon-img setprop config/offline=true && svcadm refresh falcon-img`.
- **systemd:** put `FALCON_IMG_*` settings in `/etc/default/falcon-img`.

Falcon expects port 7780 unless it's given an explicit `--img-server host:port`.

## HTTP API (v1)

| Endpoint                         | Returns                                         |
|----------------------------------|-------------------------------------------------|
| `GET /v1/discover`               | `{"service":"falcon-img","api_version":1}`      |
| `GET /v1/assets/{name}`          | `{name}` from the falcon assets bucket          |
| `GET /v1/propolis/{rev}/{file}`  | `{file}` from the propolis falcon build `{rev}` |
