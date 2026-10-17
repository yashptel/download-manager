# aria2-next

OpenDM bundles [aria2-next](https://github.com/AnInsomniacy/aria2-next) as its
download engine. It runs as a separate process and is controlled over its
JSON-RPC interface.

aria2-next is licensed under the GNU General Public License, version 2 or later.
Its complete source for the bundled version is available at
`https://github.com/AnInsomniacy/aria2-next/tree/v<VERSION>`, where `<VERSION>`
is the content of [VERSION](VERSION). The license text ships next to the
executable in every OpenDM build.

To upgrade: change `VERSION`, replace `checksums.sha256` with the lines for the
four desktop assets from that release's checksum file, run
`scripts/fetch-engine.sh` and the core's engine test
(`cargo test -p opendm-core --test engine`).
