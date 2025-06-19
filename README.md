# ![Icon of utsuru][icon-image] utsuru

[![License][license-image]][license-url]
[![CI][ci-image]][ci-url]
[![Crates.io][cratesio-image]][cratesio-url]
[![Deps.rs][depsrs-image]][depsrs-url]
[![Discord][discord-image]][discord-url]

![A screenshot of utsuru's Web UI][webui-image]

- [Overview](#overview)
- [Features](#features)
- [Installation](#installation)
- [CLI](#cli)
- [Getting started](#getting-started)
- [Adapters](#adapters)
- [Join us](#join-us)
- [License](#license)

[icon-image]: https://github.com/user-attachments/assets/e0e13ed6-0b14-42b2-a5b9-25a369d0cd1d
[license-image]: https://img.shields.io/badge/License-MIT-yellow.svg
[license-url]: https://opensource.org/licenses/MIT
[ci-image]: https://github.com/VincentVerdynanta/utsuru/workflows/CI/badge.svg
[ci-url]: https://github.com/VincentVerdynanta/utsuru/actions
[cratesio-image]: https://img.shields.io/crates/v/utsuru.svg
[cratesio-url]: https://crates.io/crates/utsuru
[depsrs-image]: https://deps.rs/repo/github/VincentVerdynanta/utsuru/status.svg
[depsrs-url]: https://deps.rs/repo/github/VincentVerdynanta/utsuru
[discord-image]: https://img.shields.io/discord/1381377249923567707?logo=discord
[discord-url]: https://discord.gg/An5jjhNUE3
[webui-image]: https://github.com/user-attachments/assets/8bc95337-8353-4188-b2a6-8af08166ff70

## Overview

utsuru is a WebRTC utility that accepts track packets from a single
source and forwards it to multiple mirrors.

Some situations you might want to use utsuru include:

* You want to broadcast to Discord without being limited to screen sharing.
* You want to broadcast a single feed to multiple Discord calls simultaneously.
* You want to use OBS to broadcast to a WebRTC remote peer that does not provide a WHIP endpoint.

## Features

* Portable and lightweight:
  * The entire application fits in a single binary.
  * Minimal CPU and memory footprint.
* Web UI and REST API for managing mirrors.
* Track packets are sent from source to mirrors as is, no transcoding.
* Discord Live is implemented using the WebRTC protocol (same as Discord web client).

## Installation

### With cargo

```text
$ cargo install utsuru
```

### Binaries on Windows, Linux, and macOS

You can download prebuilt utsuru from [Github Releases][gh-releases-url].

[gh-releases-url]: https://github.com/VincentVerdynanta/utsuru/releases

## CLI

```text
Usage: utsuru [OPTIONS]

Options:
  -h, --host <host>                Specify bind address [default: 127.0.0.1]
  -p, --port <port>                Specify port to listen on [default: 3000]
  -v, --verbosity <verbosity>      Log verbosity [default: off]
      --completions <completions>  Print shell completion script for <shell> [possible values: bash, elvish, fish, powershell, zsh]
      --help                       Print help
  -V, --version                    Print version
```

## Getting started

See [`docs/getting-started.md`][getting-started] for basic usage.

[getting-started]: ./docs/getting-started.md

## Adapters

### Mirrors

- [X] Discord Live

### Sources

- [X] WHIP

### Video codecs

- [X] H.264
- [ ] H.265
- [ ] VP8
- [ ] VP9
- [ ] AV1

## Join us

Thanks for your involvement in developing this project! We are so happy to
have you! To get started, don't hesitate to check our

* **[Discord][discord-url]:** Real-time Broadcast Discord server.

### Code of Conduct

This project has adopted the Rust Code of Conduct. Please check
[`CODE_OF_CONDUCT.md`][code-of-conduct] for more details.

The entire utsuru community is expected to abide by the
code of conduct when contributing or participating in discussions.

[code-of-conduct]: ./CODE_OF_CONDUCT.md

## License

This project is licensed under the MIT License - see the [`LICENSE`] file
for details.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in utsuru by you, shall be licensed as MIT,
without any additional terms or conditions.

[`LICENSE`]: ./LICENSE
