# Antarc(ticite) custom network protocol

**Antarc** is a custom protocol that runs on top of UDP, and supports reliable and unreliable
packets. Currently it works (just barely), but the API is unstable and untested.

## ⚠️⚠️⚠️

Consider everything you see here to be unstable, this is mostly experimental code at this point.

The project's goal is to have as much compile information as possible, so every `match`, and
`if` you see in this code are either 100% required due to the dynamic nature of what is going on, or
some missing type mangling that I've yet to tackle. No `dyn Trait` either, only static dispatch.

## Features

- Connection handshake;
- Packet reliability under your control (under construction);
- Congestion handling (planned);
- Encryption (planned);
- More to come...;

## License

[MIT](./license-mit) or [APACHE 2.0](./license-apache)
