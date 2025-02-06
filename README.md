# Unifi Access API Client

[![Crates.io][crates-badge]][crates-url]
[![Docs][docs-badge]][docs-url]
![Tests](https://github.com/Carter12s/unifi_access/actions/workflows/rust.yml/badge.svg)
[![MIT licensed][mit-badge]][mit-url]

[crates-badge]: https://img.shields.io/crates/v/unifi_access.svg
[crates-url]: https://crates.io/crates/unifi_access
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/tokio-rs/tokio/blob/master/LICENSE
[docs-badge]: https://img.shields.io/badge/docs-published-blue
[docs-url]: https://docs.rs/unifi_access/latest/unifi_access/


This is a basic handwritten wrapper for the Unifi Access API.

It is based on endpoints documented here:
https://core-config-gfoz.uid.alpha.ui.com/configs/unifi-access/api_reference.pdf

See the [docs](https://docs.rs/unifi_access/latest/unifi_access/) for more information.

## Other Unifi Clients

Unifi's APIs are split in implementation and design. This crate is focused on the Unifi API for controller door access and door locks.

 - If you are looking for a client for Unifi Network API checkout: [unifi-rs](https://github.com/CallumTeesdale/unifi-rs)
 - If you are looking for Security camera and NVR API checkout: [unifi-protect-rust](https://github.com/larsfroelich/unifi-protect-rust)

Contributions welcome!
