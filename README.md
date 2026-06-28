# FlecsECS authority plugin

<!-- northstar-badges:start -->
[![CI](https://github.com/Take-Some/FlecsECS/actions/workflows/ci.yml/badge.svg)](https://github.com/Take-Some/FlecsECS/actions/workflows/ci.yml)
[![Dependabot](https://img.shields.io/badge/Dependabot-enabled-025E8C?style=flat-square&logo=dependabot)](https://github.com/Take-Some/FlecsECS/network/updates)
![Rust](https://img.shields.io/badge/Rust-stable-F28C28?style=flat-square&logo=rust)
![Platform](https://img.shields.io/badge/Platform-Windows-0078D4?style=flat-square&logo=windows)
![Engine](https://img.shields.io/badge/Engine-North%20Star-2D9CFF?style=flat-square)
![Plugin](https://img.shields.io/badge/Plugin-FLECS%20ECS-103B4A?style=flat-square)
<!-- northstar-badges:end -->

`FlecsECS` is a runtime provider for NewEngine's gateway/override model. It is
not wired by special-case engine code: it declares backend capabilities and lets
`ActiveGatewayRegistry` select it over engine-runtime baselines.

The plugin owns one shared Flecs world and exposes it through three service
contracts:

```text
consumer/tool/runtime
  -> engine.ecs
  -> ActiveGatewayRegistry
  -> ecs.api owned by newengine.ecs.flecs
  -> shared Flecs world

consumer/tool/runtime
  -> engine.entity
  -> ActiveGatewayRegistry
  -> entity.api owned by newengine.ecs.flecs
  -> same shared Flecs world

consumer/tool/runtime
  -> engine.scene
  -> ActiveGatewayRegistry
  -> scene.api owned by newengine.ecs.flecs
  -> same shared Flecs world
```

This is the plugin-system proof, not an ECS-specific shortcut: one plugin can
become the selected authority for multiple engine gateways by descriptor facts
only. Built-in gateways remain baselines and become `shadowed` when the plugin
wins.

## Boundary

The plugin does not expose raw Flecs handles over the service boundary. It maps
NewEngine DTOs to a private Flecs world and returns stable opaque entity ids.

Current authority scope:

```text
scene load/save truth           : FlecsECS plugin via scene.api
entity identity/lifecycle truth : FlecsECS plugin via entity.api
ECS summary/snapshot truth      : FlecsECS plugin via ecs.api
semantic component packets      : FlecsECS plugin via command_json_v1
native World                    : typed component cache / hot-path staging
```

Scene bootstrap can declare its native staging cache to the provider through
`engine.entity` spawn packets and `engine.ecs` semantic component packets. That
makes `selected_player_authority` an opaque provider `EntityHandle`; native
`EntityId` is only a cache key.

## Build

```bat
Plugins\build_all_plugins.cmd FlecsECS release --force
```

The installed runtime DLL is copied to:

```text
pluginsRuntime\newengine-ecs-flecs-0.1.0-release.dll
```

`third_party/flecs/distr/flecs.c` is statically compiled into the plugin cdylib,
so no separate Flecs DLL is needed.

## Service contracts

Provider services:

```text
ecs.api
entity.api
scene.api
```

Backend capabilities:

```text
ecs.backend
entity.backend
scene.backend
```

Engine gateways:

```text
engine.ecs
engine.entity
engine.scene
```

Supported ECS methods:

```text
info_json
invoke_json
summary_json_v1
snapshot_json_v1
command_json_v1
shutdown_v1
```

Supported entity methods:

```text
info_json
invoke_json
list_json_v1
exists_json_v1
spawn_json_v1
despawn_json_v1
shutdown_v1
```

Supported scene methods:

```text
scene.formats_json
scene.load_json_v1
scene.save_json_v1
```

## Expected diagnostics

A healthy launch should show all three authority routes active from
`newengine.ecs.flecs` and all engine-runtime baselines shadowed:

```text
engine.ecs    active   first-party-plugin ecs.api    newengine.ecs.flecs ecs    ecs.backend
engine.ecs    shadowed engine-runtime       engine.ecs ...                  ecs    ecs.backend
engine.entity active   first-party-plugin entity.api newengine.ecs.flecs entity entity.backend
engine.entity shadowed engine-runtime       engine.entity ...              entity entity.backend
engine.scene  active   first-party-plugin scene.api  newengine.ecs.flecs scene  scene.backend
engine.scene  shadowed engine-runtime       engine.scene ...               scene  scene.backend
```

Expected world authority mode after bootstrap:

```text
world authority: native scene cache declared ... selected_provider=Some(...)
world authority: bootstrap ... mode='plugin-ecs-entity-authority'
```

`split-authority` should not appear while `engine.ecs`, `engine.entity` and
`engine.scene` are all owned by `newengine.ecs.flecs`.

## CI / dependency updates

- GitHub Actions workflow: `.github/workflows/ci.yml`.
- Dependabot configuration: `.github/dependabot.yml`.
- CI runs on `windows-latest` and executes `cargo check`, `cargo test`, `cargo clippy --no-deps -- -D warnings`, and a release build for the full Cargo workspace.
- The workflow checks out this plugin into `NorthStar/Plugins/FlecsECS` and checks out `Take-Some/NewEngine` into `NorthStar/NewEngine` so local path dependencies keep the same layout as the development workspace.
- If `Take-Some/NewEngine` is private, configure repository secret `NORTHSTAR_CI_TOKEN` with read access to that repository.
