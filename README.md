# FlecsECS authority plugin

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
NewEngine\neocore2\plugins\newengine-ecs-flecs-0.1.0-release.dll
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
