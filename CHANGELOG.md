# Changelog

## [0.5.1](https://github.com/evannagle/ludolph/compare/v0.5.0...v0.5.1) (2026-02-25)


### Bug Fixes

* pin Rust version for consistent CI builds ([#7](https://github.com/evannagle/ludolph/issues/7)) ([6134cc9](https://github.com/evannagle/ludolph/commit/6134cc954599859a3d0c678b4afafc34f606d227))

## [0.5.0](https://github.com/evannagle/ludolph/compare/v0.4.0...v0.5.0) (2026-02-25)


### Features

* **mcp:** add auto-formatting with black and ruff ([7f5116a](https://github.com/evannagle/ludolph/commit/7f5116a86f0fe47c03adc9d4c1da30491557e6fb))
* **mcp:** add modular 34-tool MCP server ([5d412a0](https://github.com/evannagle/ludolph/commit/5d412a02a528d7287502e53ad601af66a430b3c2))
* **mcp:** add modular Python MCP server with 10 tools ([9044f3d](https://github.com/evannagle/ludolph/commit/9044f3d8ae8e91f5ac80003c086a0267fbad385f))
* **mcp:** package MCP server as GitHub release asset ([acfe32b](https://github.com/evannagle/ludolph/commit/acfe32b3b6ae6cca59c9a16725d5bb06a866fb4a))


### Bug Fixes

* add -n flag to SSH commands to prevent stdin consumption ([68179be](https://github.com/evannagle/ludolph/commit/68179be3cfb30ef4168262c6ab1dc54ac5aade5e))
* add -T to SSH heredocs for curl|bash compatibility ([569e2b8](https://github.com/evannagle/ludolph/commit/569e2b8007877edf1e7dc2a94c4b12b41cee99ce))
* improve installer reliability with better error handling and trackable steps ([7a5468c](https://github.com/evannagle/ludolph/commit/7a5468c363243fcdd2d7ef403c9194241afbb8c4))
* replace SSH heredocs with inline commands for curl|bash compatibility ([5ce5a93](https://github.com/evannagle/ludolph/commit/5ce5a933f9ab6d22a764115064f3865c59cab8dd))
* use explicit stdin redirect (&lt; /dev/null) for SSH commands ([37a26f1](https://github.com/evannagle/ludolph/commit/37a26f1d5112d00b0ad323561c4093d93ce59f26))

## [0.4.0](https://github.com/evannagle/ludolph/compare/v0.3.0...v0.4.0) (2026-02-24)


### Features

* robust installer with Tailscale fallback, friendly bot messages ([52926e8](https://github.com/evannagle/ludolph/commit/52926e82967aec2e1120d7095f7fc8230511387d))


### Bug Fixes

* add -n to SSH test to prevent stdin consumption ([87043a1](https://github.com/evannagle/ludolph/commit/87043a1aaaa1d54e15c25a7ffc06437c2dc6d4ab))
* read from /dev/tty for curl|bash compatibility ([8363424](https://github.com/evannagle/ludolph/commit/83634247b16f392d3b397dff341dd9b2d5e25a47))
* use GitHub alert syntax for warning box ([f119333](https://github.com/evannagle/ludolph/commit/f1193336c9dd59bcdf3604eff248302f036ca335))

## [0.3.0](https://github.com/evannagle/ludolph/compare/v0.2.1...v0.3.0) (2026-02-24)


### Features

* add /release skill ([edeb36d](https://github.com/evannagle/ludolph/commit/edeb36dd513d9c5af515bfd4e1b3bd9597d6e6b6))
* add lu check command ([49e7a0d](https://github.com/evannagle/ludolph/commit/49e7a0da64f630495f7ec9d64f86f67142065512))
* add pi-digit spinner animation for scanning ([384e7bd](https://github.com/evannagle/ludolph/commit/384e7bd179adb04d1f82592fecfffc289248b932))
* add Skip status variant for lu check ([50de7f9](https://github.com/evannagle/ludolph/commit/50de7f9adde2444d62442eb1aae0219ca322b2fd))
* complete MCP architecture for Pi thin client ([6681d5b](https://github.com/evannagle/ludolph/commit/6681d5b68914c36b845be67116a7ff8d1152e687))
* Mac-first installer with vault sync ([948ca60](https://github.com/evannagle/ludolph/commit/948ca605fc3afaeea68733edb4d64c4e2e13d63e))
* make Syncthing setup optional ([49d4fbe](https://github.com/evannagle/ludolph/commit/49d4fbe73bd3c9fb78ad3d4f87161ddb2e09a7d6))
* offer GitHub setup when no version control exists ([12c5447](https://github.com/evannagle/ludolph/commit/12c5447667fadea3d3284a3cdb4bd3b4f4050598))
* replace git sync with Syncthing for real-time vault sync ([9199e4e](https://github.com/evannagle/ludolph/commit/9199e4ecd629fe25dfd067a6be832cffce6b682a))


### Bug Fixes

* add bullet points to file list ([0b13229](https://github.com/evannagle/ludolph/commit/0b13229b70bf0ddb0e50e4d05479a374d9bca996))
* clarify vault path prompt is local to this machine ([c4a1dbe](https://github.com/evannagle/ludolph/commit/c4a1dbe5188da9a88e273321f8f30b8c5a2b2ad9))
* clean up installer UX with clear steps and proper prompts ([7e39908](https://github.com/evannagle/ludolph/commit/7e39908b8e426d3580aeb4a0ff7854509819dc5b))
* consistent spinners, spacing, and bullet character ([776c4db](https://github.com/evannagle/ludolph/commit/776c4dbc76f6e73e985cca572341f1aeb4b6a016))
* handle subdirectory vaults and bidirectional sync ([7949ea9](https://github.com/evannagle/ludolph/commit/7949ea90452283e3236eaafef0a1c3325046be44))
* installer prompt style and vault path validation ([312190a](https://github.com/evannagle/ludolph/commit/312190ae1e4d5690508fc0521c87e9a30f9f3338))
* per-file exclusion and use red alert for sensitive files ([3178191](https://github.com/evannagle/ludolph/commit/3178191c4b89d0f67e183c94582b73ebc5fe03f7))
* show deployment method explicitly in Step 6 ([ada2586](https://github.com/evannagle/ludolph/commit/ada2586725e23cecfa02244460b2eb551f641cfb))
* source cargo env in release command SSH calls ([a86c2a6](https://github.com/evannagle/ludolph/commit/a86c2a6faa42773aad44f76aed98d92690baee49))
* use bouncing ball spinner to match Rust version ([a48f74d](https://github.com/evannagle/ludolph/commit/a48f74d46846bed920242a8422667c23742f92e7))
* use git clone when available, add progress steps for vault copy ([b178eab](https://github.com/evannagle/ludolph/commit/b178eabb1d147a53b61ec0b864901b32c056d4d7))
* use PI_VAULT_PATH for Syncthing config on Pi ([a165feb](https://github.com/evannagle/ludolph/commit/a165febde0423c1d3939c50b3902374eaf1f55a0))
* use spinners for all progress indicators ([bdc2f10](https://github.com/evannagle/ludolph/commit/bdc2f104da49339b165b0f41fd1292a0ab7b76b4))

## [0.2.1](https://github.com/evannagle/ludolph/compare/v0.2.0...v0.2.1) (2026-02-22)


### Bug Fixes

* collapse nested if in create_file ([31359e0](https://github.com/evannagle/ludolph/commit/31359e05090acf3883d586897fac237684a9f7d5))
* use rustls for teloxide to enable ARM cross-compile ([ac2e2a6](https://github.com/evannagle/ludolph/commit/ac2e2a614e4acefb67aa92def46094caf28556a2))

## [0.2.0](https://github.com/evannagle/ludolph/compare/v0.1.0...v0.2.0) (2026-02-22)


### Features

* idempotent installer with clear steps and options ([82ade79](https://github.com/evannagle/ludolph/commit/82ade793bb3d75c8063eae00982772498c415168))
* streamlined one-liner install with auto Pi detection ([047c56b](https://github.com/evannagle/ludolph/commit/047c56bd78326b3cf614cae73511edcd5ee808d9))

## 0.1.0 (2026-02-22)


### Features

* add /branch and /pr skills, update branch naming docs ([10b13f2](https://github.com/evannagle/ludolph/commit/10b13f24add92c120a65c5096b01c6a2968ec7c4))
* add CLI style guide with Pi-themed UI ([1c544a9](https://github.com/evannagle/ludolph/commit/1c544a9a0cd7c056ac4e96bfe08bee1d16eb09e9))
* add Pi SSH configuration to setup wizard ([2742179](https://github.com/evannagle/ludolph/commit/27421793651722b3495fe92147a6681b376bc03a))
* git-first sync setup with safety checks ([668e8cc](https://github.com/evannagle/ludolph/commit/668e8cc8d27e8392d5aab0951c35522cc719e70b))
* improve setup UX with ponging spinner and API validation ([8576537](https://github.com/evannagle/ludolph/commit/8576537256e1ecd9085224eecdf6f9bf0ac3bc7d))
* use rustls for cross-platform builds ([43bcb69](https://github.com/evannagle/ludolph/commit/43bcb6904dd7e1a341d57ccf9ffc12d47e51ecff))


### Bug Fixes

* collapse nested if statements for clippy ([2f97aee](https://github.com/evannagle/ludolph/commit/2f97aee698abe9907432035569d9e5ce4766c60c))
* collapse remaining nested if statements ([0a72f8a](https://github.com/evannagle/ludolph/commit/0a72f8a6af7ef1554eb45667e9eee58da5ab2c13))
* use develop branch URL for pi-setup docs ([66c24e1](https://github.com/evannagle/ludolph/commit/66c24e13cfb9e06f39f4976a10e6cc696e77aa54))
* use production branch URL for pi-setup docs ([c059ca0](https://github.com/evannagle/ludolph/commit/c059ca0ff821ce557467580436424b26c3436557))
