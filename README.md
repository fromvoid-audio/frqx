# frqx

frqx is a macOS VST3 plug-in for designing and auditioning microtonal scales. It publishes a 128-key tuning table to MTS-ESP instruments and includes a built-in MIDI-driven reference tone.

- Edit scales from 3 to 48 notes by dragging pitches or entering values in hertz and cents.
- Explore related scales in Pitch Space, then set the root, repeat interval, and ratio snapping.
- Save scales in eight local slots or recall tuning and audition settings with your DAW project.

## Build

Requires macOS (arm64), Rust, and the [Steinberg VST3 SDK](https://github.com/steinbergmedia/vst3sdk) (`VST3_SDK_DIR`).

```sh
cargo build --release --locked
VST3_SDK_DIR=/path/to/vst3sdk bash scripts/dist.sh  # run checks and build dist/frqx-*.vst3
```

MTS-ESP uses ODDSound’s shared system library. frqx was previously called Freqs and retains its VST3 class IDs and local scale slots.

## About

I’m [PORTALSURFER](https://soundcloud.com/portalsurfer). I make music and build instruments and tools for making it under [fromvoid](https://github.com/fromvoid-audio). I use AI extensively in that work and share these projects so more people can make music, whatever their budget.

I build this software for myself first and foremost and offer no guarantees of support. That said, I welcome code, documentation, and interesting ideas, and I’m happy to add them when we align on the direction.

Copyright wasn't just inconvenienced by AI; perhaps it was always the wrong bet.

We treated ideas like property, then acted surprised when progress slowed and wealth pooled upward. But evolution has always followed a simple pattern: copying, mutation, and selection. Ideas, like living things, develop through variation and iteration.

Perhaps the mistake was never copying, but our insistence on preventing it.

Soon, software will be something we simply wish into existence, endlessly adapting to our needs. When creation becomes abundant, artificial scarcity loses its purpose.

fromvoid is built in the open, not as charity, but as a refusal to restrict the evolution of ideas.

Copy. Mutate. Select. Repeat.

## Support

If you’d like to help cover development costs, you can chip in on Ko-fi:

<a href='https://ko-fi.com/F7X2225YCF' target='_blank'><img height='36' style='border:0px;height:36px;' src='https://storage.ko-fi.com/cdn/kofi6.png?v=6' border='0' alt='Buy Me a Coffee at ko-fi.com' /></a>

---

[User guide](docs/USER-GUIDE.md) · [Development](docs/DEVELOPMENT.md) · [Release tooling](docs/PLUGIN-RELEASE.md) · [MIT license](LICENSE)
