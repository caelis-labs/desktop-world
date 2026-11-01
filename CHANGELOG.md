# Changelog

## [0.2.2](https://github.com/caelis-labs/desktop-world/compare/v0.2.1...v0.2.2) (2026-11-01)


### Bug Fixes

* expire idle Windows cursor feedback ([#85](https://github.com/caelis-labs/desktop-world/issues/85)) ([6e61f4c](https://github.com/caelis-labs/desktop-world/commit/6e61f4c54a3ea4536b726f6bc87d4839fe15fb31))
* stabilize Windows desktop input and agent plugin installs ([#83](https://github.com/caelis-labs/desktop-world/issues/83)) ([1f16e6e](https://github.com/caelis-labs/desktop-world/commit/1f16e6e5fc3e28ac97a120711d72e8a8713b533d))

## [0.2.1](https://github.com/caelis-labs/desktop-world/releases/tag/v0.2.1) (2026-11-01)

### Features

* Target supported Windows client-area mouse messages without moving the shared cursor; automatically raise the target for foreground-required input.
* Keep session cursor feedback and exact target, DPI, cancellation and input-lease checks.

### Fixes

* Retain an automatically raised target in front and fit macOS captures across mixed-scale displays.
* Select the latest stable package in Agent installation prompts and download links.

The exact platform sources, GUI acceptance and retained Agent failures are in
[the release record](docs/releases/0.2.1.md) and its RELEASE.json.

## [0.2.0](https://github.com/caelis-labs/desktop-world/releases/tag/v0.2.0) (2026-11-01)

* Ship the standard MCP and Agent Skill rewrite for macOS arm64 and Windows x64,
  including native desktop actions, bounded discovery, inspectable receipts and
  session cursor feedback. See [the release scope](docs/releases/0.2.0.md).

## [0.1.3](https://github.com/caelis-labs/desktop-world/compare/v0.1.2...v0.1.3) (2026-10-25)


### Bug Fixes

* repair CUA input and simplify browser workflows and approval ([#47](https://github.com/caelis-labs/desktop-world/issues/47)) ([8fe6910](https://github.com/caelis-labs/desktop-world/commit/8fe69107db4a7a37b2b3cb093a7d183f33daeecb))

## [0.1.2](https://github.com/caelis-labs/desktop-world/compare/v0.1.1...v0.1.2) (2026-10-20)


### Bug Fixes

* align click contracts with CUA 0.37.0 ([#43](https://github.com/caelis-labs/desktop-world/issues/43)) ([d455eb9](https://github.com/caelis-labs/desktop-world/commit/d455eb9353d9d60c5d3de8358b3f5f2ec041a0d2))

## [0.1.1](https://github.com/caelis-labs/desktop-world/compare/v0.1.0...v0.1.1) (2026-10-20)


### Features

* document Agent installation and automate DTW releases ([a60710f](https://github.com/caelis-labs/desktop-world/commit/a60710f8d264aa85e16e71d0686a22810a6c3a86))
* **skills:** optimize desktop-world skill description and SOP structure ([#38](https://github.com/caelis-labs/desktop-world/issues/38)) ([e6e4a2a](https://github.com/caelis-labs/desktop-world/commit/e6e4a2a3ec86453e8d1b4ecf3ff440ee47a3e5cd))
