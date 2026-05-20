# FlecsECS provider plugin

`FlecsECS` is a runtime provider for NewEngine's gateway/override model. It is
not wired by special-case engine code: it declares backend capabilities and lets
`ActiveGatewayRegistry` select it over engine-owned baselines.

The plugin now owns one shared Flecs world and exposes it through two service
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
```

This makes the plugin a stronger replacement proof: `engine.ecs` summary/snapshot
and `engine.entity` lifecycle/list/exists calls observe the same provider-owned
truth instead of two separate worlds.

## Boundary

The plugin does not expose raw Flecs handles over the service boundary. It maps
NewEngine DTOs to a private Flecs world and returns stable opaque entity ids.

Current authority scope:

```text
entity identity/lifecycle truth : FlecsECS plugin
ECS summary/snapshot truth      : FlecsECS plugin
component schema truth          : provider-local until a typed component schema API exists
scene/render/physics gameplay   : still consume their current typed runtime world until migrated
```

The important invariant is that built-in `engine.ecs` and `engine.entity` routes
should become `shadowed` when this first-party plugin is present. That confirms
plugin override priority, not filename or hand-written branch logic.

## Build

```bat
Plugins\build_all_plugins.cmd FlecsECS release --force
```

The installed runtime DLL is copied to:

```text
NewEngine\neocore2\plugins\newengine-ecs-flecs-0.1.0-release.dll
```

`third_party/flecs/distr/flecs.c` is statically compiled into the plugin cdylib,
so no separate Flecs DLL is needed.

## Service contracts

Provider services:

```text
ecs.api
entity.api
```

Backend capabilities:

```text
ecs.backend
entity.backend
```

Engine gateways:

```text
engine.ecs
engine.entity
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

## Expected diagnostics

A healthy launch should show both routes active from `newengine.ecs.flecs` and
both engine-owned baselines shadowed:

```text
engine.ecs    active   first-party-plugin ecs.api    newengine.ecs.flecs ecs    ecs.backend
engine.ecs    shadowed engine-owned       engine.ecs ...                  ecs    ecs.backend
engine.entity active   first-party-plugin entity.api newengine.ecs.flecs entity entity.backend
engine.entity shadowed engine-owned       engine.entity ...              entity entity.backend
```
