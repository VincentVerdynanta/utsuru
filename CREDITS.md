# CREDITS

This document contains licensing informations and credits for the
third-party works relevant to this project.

## Adapted code

* **The implementation of SampleBuilder (src/utils/io.rs)**
  * Copyright © 2021 WebRTC.rs, licensed under the standard MIT terms  
    Source: https://github.com/webrtc-rs/webrtc/blob/master/media/src/io/sample_builder/mod.rs
  * Copyright © 2022 Martin Algesten, licensed under the standard MIT terms  
    Source: https://github.com/algesten/str0m/blob/main/src/packet/buffer_rx.rs
* **The implementation of H264 Depacketizer (src/utils/codecs.rs)**
  * Copyright © 2021 WebRTC.rs, licensed under the standard MIT terms  
    Source: https://github.com/webrtc-rs/webrtc/blob/master/rtp/src/codecs/h264/mod.rs
  * Copyright © 2022 Martin Algesten, licensed under the standard MIT terms  
    Source: https://github.com/algesten/str0m/blob/main/src/packet/h264.rs

## Inspiration

We would like to thank:

* The [Discord-video-stream][discord-video-stream-url] project which
  gave the idea for writing an alternative implementation using the
  standard WebRTC protocol instead of Discord's custom UDP protocol.
* The [BitWHIP][bitwhip-url] project which gave the idea to use WHIP to
  receive broadcast from OBS.
* The [Tuxphones][tuxphones-url] project which gave the idea of reverse
  engineering the Discord web client for understanding the process of
  connecting to the voice server endpoint through WebRTC protocol.

[discord-video-stream-url]: https://github.com/Discord-RE/Discord-video-stream
[bitwhip-url]: https://github.com/bitwhip/bitwhip
[tuxphones-url]: https://github.com/ImTheSquid/Tuxphones
