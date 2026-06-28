#![forbid(unsafe_op_in_unsafe_fn)]

use std::collections::{BTreeMap, BTreeSet};
use std::os::raw::{c_float, c_int};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use abi_stable::erased_types::TD_Opaque;
use abi_stable::std_types::{RResult, RString, RVec};
use newengine_ecs_api::{
    EcsCommand, EcsCommandRequest, EcsCommandResponse, EcsCommandResult, EcsInvokeRequest,
    EcsServiceInfo, EcsSnapshotRequest, EcsWorldSnapshot, EcsWorldSummary,
    ECS_BACKEND_CAPABILITY_ID, ECS_BACKEND_SERVICE_SPEC, ECS_REQUIRED_METHODS_V1,
    ECS_SERVICE_ID, ECS_SERVICE_METHOD_COMMAND_JSON_V1, ECS_SERVICE_METHOD_INFO,
    ECS_SERVICE_METHOD_INVOKE, ECS_SERVICE_METHOD_SHUTDOWN_V1,
    ECS_SERVICE_METHOD_SNAPSHOT_JSON_V1, ECS_SERVICE_METHOD_SUMMARY_JSON_V1,
    ENGINE_ECS_SERVICE_ID,
};
use newengine_entity_api::{
    EntityDespawnRequest, EntityDespawnResponse, EntityDespawnResult, EntityExistsRequest,
    EntityExistsResponse, EntityHandle, EntityInvokeRequest, EntityListRequest,
    EntityListResponse, EntityRecord, EntityServiceInfo, EntitySpawnRequest, EntitySpawnResponse,
    ENGINE_ENTITY_SERVICE_ID, ENTITY_BACKEND_CAPABILITY_ID, ENTITY_BACKEND_SERVICE_SPEC,
    ENTITY_REQUIRED_METHODS_V1, ENTITY_SERVICE_ID, ENTITY_SERVICE_METHOD_DESPAWN_JSON_V1,
    ENTITY_SERVICE_METHOD_EXISTS_JSON_V1, ENTITY_SERVICE_METHOD_INFO,
    ENTITY_SERVICE_METHOD_INVOKE, ENTITY_SERVICE_METHOD_LIST_JSON_V1,
    ENTITY_SERVICE_METHOD_SHUTDOWN_V1, ENTITY_SERVICE_METHOD_SPAWN_JSON_V1,
};
use newengine_assets_api::AssetServiceClient;
use newengine_scene_io::{
    method as scene_method, ENGINE_SCENE_SERVICE_ID, SCENE_BACKEND_CAPABILITY_ID,
    SCENE_BACKEND_SERVICE_SPEC, SCENE_REQUIRED_METHODS, SCENE_SERVICE_ID,
};
use newengine_plugin_api::prelude::*;

pub const FLECS_ECS_PLUGIN_ID: &str = "engine.ecs.constellation";
pub const FLECS_ECS_PLUGIN_NAME: &str = "Constellation ECS Authority";
pub const FLECS_ECS_PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FLECS_BACKEND_ID: &str = "constellation";
const ECS_PROVIDER_GATEWAY_ID: &str = "engine.ecs.constellation";
const ENTITY_PROVIDER_GATEWAY_ID: &str = "engine.entity.constellation";
const SCENE_PROVIDER_GATEWAY_ID: &str = "engine.scene.constellation";

const DEFAULT_SETTINGS_JSON: &str = r#"{
  "debug_text": "North Star | Constellation ECS/entity authority backend",
  "initial_entity_capacity": 4096,
  "minimal_world": true,
  "progress_on_advance_tick": false
}"#;

const ENTITY_SPAWN_HARD_LIMIT: usize = 65_536;

#[repr(C)]
struct ecs_world_t {
    _private: [u8; 0],
}

type EcsEntityT = u64;
type EcsFTimeT = c_float;

extern "C" {
    fn ecs_init() -> *mut ecs_world_t;
    fn ecs_mini() -> *mut ecs_world_t;
    fn ecs_fini(world: *mut ecs_world_t) -> c_int;
    fn ecs_dim(world: *mut ecs_world_t, entity_count: c_int);
    fn ecs_new(world: *mut ecs_world_t) -> EcsEntityT;
    fn ecs_delete(world: *mut ecs_world_t, entity: EcsEntityT);
    fn ecs_is_alive(world: *const ecs_world_t, entity: EcsEntityT) -> bool;
    fn ecs_get_alive(world: *const ecs_world_t, entity: EcsEntityT) -> EcsEntityT;
    fn ecs_progress(world: *mut ecs_world_t, delta_time: EcsFTimeT) -> bool;
}

#[derive(Debug, Clone)]
struct FlecsEcsPluginConfig {
    debug_text: String,
    initial_entity_capacity: i32,
    minimal_world: bool,
    progress_on_advance_tick: bool,
}

impl Default for FlecsEcsPluginConfig {
    fn default() -> Self {
        Self {
            debug_text: "North Star | Constellation ECS/entity authority backend".to_owned(),
            initial_entity_capacity: 4096,
            minimal_world: true,
            progress_on_advance_tick: false,
        }
    }
}

#[derive(Default)]
struct FlecsEcsPlugin {
    enabled: bool,
}

impl FlecsEcsPlugin {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor::builder(
            FLECS_ECS_PLUGIN_ID,
            FLECS_ECS_PLUGIN_NAME,
            FLECS_ECS_PLUGIN_VERSION,
            PluginKind::Runtime,
        )
        .provides_service(
            ECS_SERVICE_ID,
            1,
            r#"{"role":"ecs-authority-bridge","contract":"ecs.api","gateway":"engine.ecs.constellation","root_gateway":"engine.ecs","backend":"constellation","implementation":"flecs","shared_truth":"flecs-world"}"#,
        )
        .provides_service(
            ENTITY_SERVICE_ID,
            1,
            r#"{"role":"entity-authority-bridge","contract":"entity.api","gateway":"engine.entity.constellation","root_gateway":"engine.entity","backend":"constellation","implementation":"flecs","shared_truth":"flecs-world"}"#,
        )
        .provides_service(
            SCENE_SERVICE_ID,
            1,
            r#"{"role":"scene-authority-bridge","contract":"scene.api","gateway":"engine.scene.constellation","root_gateway":"engine.scene","backend":"constellation","implementation":"flecs","shared_truth":"flecs-world"}"#,
        )
        .push(CapabilityDesc::backend_route(
            ECS_BACKEND_CAPABILITY_ID,
            BackendRouteDescriptor::new(ECS_BACKEND_SERVICE_SPEC)
                .provider_route(ECS_PROVIDER_GATEWAY_ID)
                .backend(FLECS_BACKEND_ID)
                .metadata_json("implementation", serde_json::json!("flecs"))
                .priority(500)
                .features([
                    "gateway-summary",
                    "entity-snapshot",
                    "command-envelope",
                    "flecs-world",
                    "flecs-id-allocation",
                    "shared-entity-authority",
                ])
                .system_tags([
                    "ecs",
                    "provider",
                    "replaceable-backend",
                    "plugin-override-proof",
                    "single-source-of-truth",
                ]),
        ))
        .push(CapabilityDesc::backend_route(
            ENTITY_BACKEND_CAPABILITY_ID,
            BackendRouteDescriptor::new(ENTITY_BACKEND_SERVICE_SPEC)
                .provider_route(ENTITY_PROVIDER_GATEWAY_ID)
                .backend(FLECS_BACKEND_ID)
                .metadata_json("implementation", serde_json::json!("flecs"))
                .priority(500)
                .features([
                    "opaque-stable-handles",
                    "entity-list",
                    "entity-exists",
                    "entity-lifecycle",
                    "flecs-world",
                    "shared-ecs-authority",
                ])
                .system_tags([
                    "entity",
                    "provider",
                    "replaceable-backend",
                    "plugin-override-proof",
                    "single-source-of-truth",
                ]),
        ))
        .push(CapabilityDesc::backend_route(
            SCENE_BACKEND_CAPABILITY_ID,
            BackendRouteDescriptor::new(SCENE_BACKEND_SERVICE_SPEC)
                .provider_route(SCENE_PROVIDER_GATEWAY_ID)
                .backend(FLECS_BACKEND_ID)
                .metadata_json("implementation", serde_json::json!("flecs"))
                .priority(500)
                .features([
                    "scene-load-save",
                    "scene-authority",
                    "semantic-component-packets",
                    "shared-ecs-authority",
                    "shared-entity-authority",
                    "single-source-of-truth",
                ])
                .system_tags([
                    "scene",
                    "provider",
                    "replaceable-backend",
                    "plugin-override-proof",
                    "single-source-of-truth",
                ]),
        ))
        .build()
    }

    fn init_services(&mut self, host: HostApiV1, config: FlecsEcsPluginConfig) -> RResult<(), RString> {
        let backend = match FlecsWorld::new(&config) {
            Ok(backend) => Arc::new(Mutex::new(backend)),
            Err(e) => return RResult::RErr(RString::from(e)),
        };

        let ecs_service = ServiceV1_TO::from_value(
            FlecsEcsService::new(backend.clone(), config.clone()),
            TD_Opaque,
        );
        match (host.register_service_v1)(ecs_service) {
            RResult::ROk(()) => {}
            RResult::RErr(e) => return RResult::RErr(e),
        }

        let entity_service = ServiceV1_TO::from_value(
            FlecsEntityService::new(backend.clone(), config.clone()),
            TD_Opaque,
        );
        match (host.register_service_v1)(entity_service) {
            RResult::ROk(()) => {}
            RResult::RErr(e) => return RResult::RErr(e),
        }

        let scene_service = ServiceV1_TO::from_value(
            FlecsSceneService::new(backend, host.clone(), config.clone()),
            TD_Opaque,
        );
        match (host.register_service_v1)(scene_service) {
            RResult::ROk(()) => {
                log::info!(
                    "flecs ecs plugin: services registered ecs='{}' entity='{}' scene='{}' routes='{},{},{}' backend='{}' priority=500 authority='shared-constellation-world'",
                    ECS_SERVICE_ID,
                    ENTITY_SERVICE_ID,
                    SCENE_SERVICE_ID,
                    ECS_PROVIDER_GATEWAY_ID,
                    ENTITY_PROVIDER_GATEWAY_ID,
                    SCENE_PROVIDER_GATEWAY_ID,
                    FLECS_BACKEND_ID,
                );
                self.enabled = true;
                RResult::ROk(())
            }
            RResult::RErr(e) => RResult::RErr(e),
        }
    }
}

impl PluginModule for FlecsEcsPlugin {
    fn descriptor(&self) -> PluginDescriptor { Self::descriptor() }

    fn config_defaults(&self) -> RResult<ConfigBlobV1, RString> {
        RResult::ROk(ConfigBlobV1 {
            content_type: "application/json".into(),
            bytes: DEFAULT_SETTINGS_JSON.as_bytes().to_vec().into(),
            format_version: 1,
        })
    }

    fn config_apply_patches(
        &self,
        base: &ConfigBlobV1,
        patches: RVec<ConfigPatchV1>,
    ) -> RResult<ConfigApplyResultV1, RString> {
        let mut effective = match parse_json_object(base.bytes.as_slice(), "flecs ecs defaults") {
            Ok(v) => v,
            Err(e) => return RResult::RErr(RString::from(e)),
        };
        for patch in patches.iter() {
            let patch_value = match parse_json_object(patch.bytes.as_slice(), "flecs ecs patch") {
                Ok(v) => v,
                Err(e) => return RResult::RErr(RString::from(e)),
            };
            merge_json_replace(&mut effective, &patch_value);
        }
        if let Err(e) = parse_backend_config_value(&effective) {
            return RResult::RErr(RString::from(e));
        }
        let bytes = match serde_json::to_vec_pretty(&effective) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(RString::from(e.to_string())),
        };
        RResult::ROk(ConfigApplyResultV1 {
            effective: ConfigBlobV1 {
                content_type: "application/json".into(),
                bytes: bytes.into(),
                format_version: 1,
            },
            diags: RVec::new(),
            changed: true,
        })
    }

    fn config_supports_live_update(&self) -> bool { false }

    fn config_update_live(&mut self, _effective: &ConfigBlobV1) -> RResult<RVec<ConfigDiagV1>, RString> {
        RResult::ROk(RVec::new())
    }

    fn init(&mut self, host: HostApiV1, effective: ConfigBlobV1) -> RResult<(), RString> {
        let config = match parse_backend_config_blob(&effective) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(RString::from(e)),
        };
        self.init_services(host, config)
    }

    fn start(&mut self) -> RResult<(), RString> { RResult::ROk(()) }
    fn fixed_update(&mut self, _dt: f32) -> RResult<(), RString> { RResult::ROk(()) }
    fn update(&mut self, _dt: f32) -> RResult<(), RString> { RResult::ROk(()) }
    fn render(&mut self, _dt: f32) -> RResult<(), RString> { RResult::ROk(()) }
    fn shutdown(&mut self) { self.enabled = false; }
}


struct FlecsWorld {
    raw: NonNull<ecs_world_t>,
    tick: u64,
    entities_changed_tick: u64,
    alive_entities: BTreeSet<u64>,
    semantic_components: BTreeMap<u64, BTreeMap<String, serde_json::Value>>,
    loaded_scene: Option<FlecsSceneState>,
    progress_on_advance_tick: bool,
}

#[derive(Clone, Debug, Default)]
struct FlecsSceneState {
    source_path: Option<String>,
    schema: Option<String>,
    version: Option<u64>,
    title: Option<String>,
    entity_count: usize,
    entity_handles: Vec<EntityHandle>,
}

unsafe impl Send for FlecsWorld {}

impl FlecsWorld {
    fn new(config: &FlecsEcsPluginConfig) -> Result<Self, String> {
        let raw = if config.minimal_world {
            unsafe { ecs_mini() }
        } else {
            unsafe { ecs_init() }
        };
        let Some(raw) = NonNull::new(raw) else {
            return Err("flecs ecs: ecs_init/ecs_mini returned null world".to_owned());
        };
        if config.initial_entity_capacity > 0 {
            unsafe { ecs_dim(raw.as_ptr(), config.initial_entity_capacity) };
        }
        Ok(Self {
            raw,
            tick: 0,
            entities_changed_tick: 0,
            alive_entities: BTreeSet::new(),
            semantic_components: BTreeMap::new(),
            loaded_scene: None,
            progress_on_advance_tick: config.progress_on_advance_tick,
        })
    }

    fn summary(&mut self) -> EcsWorldSummary {
        self.prune_dead_entities();
        EcsWorldSummary {
            tick: self.tick,
            entity_count: self.alive_entities.len() as u64,
            storage_count: self.unique_component_type_count() as u64,
            resource_count: if self.loaded_scene.is_some() { 1 } else { 0 },
            entities_changed_tick: self.entities_changed_tick,
        }
    }

    fn snapshot(&mut self, req: EcsSnapshotRequest) -> EcsWorldSnapshot {
        let summary = self.summary();
        let mut entities = Vec::new();
        let mut truncated = false;
        if req.include_entities {
            for stable_id in self.alive_entities.iter().copied() {
                if entities.len() >= req.entity_limit {
                    truncated = true;
                    break;
                }
                entities.push(newengine_ecs_api::EcsEntitySnapshot { handle: EntityHandle::new(stable_id) });
            }
        }
        EcsWorldSnapshot { summary, entities, truncated }
    }

    fn command(&mut self, req: EcsCommandRequest) -> EcsCommandResponse {
        let mut results = Vec::with_capacity(req.commands.len());
        for (index, command) in req.commands.into_iter().enumerate() {
            match command {
                EcsCommand::SetTick { tick } => {
                    self.tick = tick;
                    results.push(EcsCommandResult {
                        index,
                        ok: true,
                        entity_id: None,
                        tick: self.tick,
                        message: "tick set by Flecs authority".to_owned(),
                    });
                }
                EcsCommand::AdvanceTick => {
                    self.tick = self.tick.saturating_add(1);
                    if self.progress_on_advance_tick {
                        let _ = unsafe { ecs_progress(self.raw.as_ptr(), 0.0) };
                    }
                    results.push(EcsCommandResult {
                        index,
                        ok: true,
                        entity_id: None,
                        tick: self.tick,
                        message: "tick advanced by Flecs authority".to_owned(),
                    });
                }
                EcsCommand::SpawnEmpty => {
                    let stable_id = self.spawn_empty();
                    results.push(EcsCommandResult {
                        index,
                        ok: true,
                        entity_id: Some(stable_id),
                        tick: self.tick,
                        message: "empty entity spawned in shared Flecs world".to_owned(),
                    });
                }
                EcsCommand::SetComponentJson { entity_id, component_type, payload } => {
                    match self.set_component_json(entity_id, component_type.clone(), payload) {
                        Ok(()) => results.push(EcsCommandResult {
                            index,
                            ok: true,
                            entity_id: Some(entity_id),
                            tick: self.tick,
                            message: format!("semantic component packet '{}' set", component_type),
                        }),
                        Err(message) => results.push(EcsCommandResult {
                            index,
                            ok: false,
                            entity_id: Some(entity_id),
                            tick: self.tick,
                            message,
                        }),
                    }
                }
                EcsCommand::RemoveComponentJson { entity_id, component_type } => {
                    match self.remove_component_json(entity_id, component_type.clone()) {
                        Ok(()) => results.push(EcsCommandResult {
                            index,
                            ok: true,
                            entity_id: Some(entity_id),
                            tick: self.tick,
                            message: format!("semantic component packet '{}' removed", component_type),
                        }),
                        Err(message) => results.push(EcsCommandResult {
                            index,
                            ok: false,
                            entity_id: Some(entity_id),
                            tick: self.tick,
                            message,
                        }),
                    }
                }
            }
        }
        let summary = self.summary();
        EcsCommandResponse { ok: true, summary, results }
    }

    fn list_entities(&mut self, limit: usize) -> EntityListResponse {
        self.prune_dead_entities();
        let mut entities = Vec::new();
        let mut truncated = false;
        for stable_id in self.alive_entities.iter().copied() {
            if entities.len() >= limit {
                truncated = true;
                break;
            }
            entities.push(EntityRecord::alive(EntityHandle::new(stable_id)));
        }
        EntityListResponse { entities, truncated, total_count: self.alive_entities.len() as u64 }
    }

    fn exists_entity(&mut self, entity: EntityHandle) -> bool {
        self.prune_dead_entities();
        entity.stable_id != 0 && self.alive_entities.contains(&entity.stable_id)
    }

    fn spawn_entities(&mut self, count: usize) -> EntitySpawnResponse {
        let count = count.min(ENTITY_SPAWN_HARD_LIMIT);
        let mut entities = Vec::with_capacity(count);
        for _ in 0..count {
            let stable_id = self.spawn_empty();
            entities.push(EntityRecord::alive(EntityHandle::new(stable_id)));
        }
        let total_count = self.summary().entity_count;
        EntitySpawnResponse { entities, tick: self.tick, total_count }
    }

    fn despawn_entities(&mut self, req: EntityDespawnRequest) -> EntityDespawnResponse {
        let mut ok = true;
        let mut results = Vec::with_capacity(req.entities.len());
        for handle in req.entities {
            let exists = self.exists_entity(handle);
            if exists {
                unsafe { ecs_delete(self.raw.as_ptr(), handle.stable_id) };
                self.alive_entities.remove(&handle.stable_id);
                self.semantic_components.remove(&handle.stable_id);
                self.entities_changed_tick = self.tick;
                results.push(EntityDespawnResult {
                    entity: handle,
                    ok: true,
                    message: "entity despawned from shared Flecs world".to_owned(),
                });
            } else {
                ok = false;
                results.push(EntityDespawnResult {
                    entity: handle,
                    ok: false,
                    message: "entity does not exist in shared Flecs world".to_owned(),
                });
            }
        }
        let total_count = self.summary().entity_count;
        EntityDespawnResponse { ok, results, tick: self.tick, total_count }
    }

    fn spawn_empty(&mut self) -> u64 {
        let entity = unsafe { ecs_new(self.raw.as_ptr()) };
        let stable_id = unsafe { ecs_get_alive(self.raw.as_ptr(), entity) };
        let stable_id = if stable_id == 0 { entity } else { stable_id };
        self.alive_entities.insert(stable_id);
        self.entities_changed_tick = self.tick;
        stable_id
    }

    fn set_component_json(
        &mut self,
        entity_id: u64,
        component_type: String,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        let component_type = component_type.trim().to_owned();
        if component_type.is_empty() {
            return Err("component_type must not be empty".to_owned());
        }
        self.prune_dead_entities();
        if entity_id == 0 || !self.alive_entities.contains(&entity_id) {
            return Err(format!("entity {entity_id} is not alive in shared Flecs world"));
        }
        self.semantic_components
            .entry(entity_id)
            .or_default()
            .insert(component_type, payload);
        self.entities_changed_tick = self.tick;
        Ok(())
    }

    fn remove_component_json(&mut self, entity_id: u64, component_type: String) -> Result<(), String> {
        let component_type = component_type.trim();
        if component_type.is_empty() {
            return Err("component_type must not be empty".to_owned());
        }
        self.prune_dead_entities();
        if entity_id == 0 || !self.alive_entities.contains(&entity_id) {
            return Err(format!("entity {entity_id} is not alive in shared Flecs world"));
        }
        if let Some(components) = self.semantic_components.get_mut(&entity_id) {
            components.remove(component_type);
            if components.is_empty() {
                self.semantic_components.remove(&entity_id);
            }
        }
        self.entities_changed_tick = self.tick;
        Ok(())
    }

    fn unique_component_type_count(&self) -> usize {
        let mut names = BTreeSet::new();
        for components in self.semantic_components.values() {
            names.extend(components.keys().cloned());
        }
        names.len()
    }

    fn load_scene_value(
        &mut self,
        scene_value: serde_json::Value,
        source_path: Option<String>,
        replace: bool,
    ) -> FlecsSceneLoadResult {
        if replace {
            self.shutdown();
        }

        let normalized = normalize_scene_payload(scene_value);
        let schema = normalized
            .get("schema")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let version = normalized.get("version").and_then(serde_json::Value::as_u64);
        let title = normalized
            .get("title")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let records = extract_scene_entity_records(&normalized);

        let mut entity_handles = Vec::with_capacity(records.len());
        for (index, record) in records.into_iter().enumerate() {
            let stable_id = self.spawn_empty();
            let handle = EntityHandle::new(stable_id);
            self.semantic_components
                .entry(stable_id)
                .or_default()
                .insert("newengine.scene.entity".to_owned(), record);
            self.semantic_components
                .entry(stable_id)
                .or_default()
                .insert(
                    "newengine.scene.source_index".to_owned(),
                    serde_json::json!({ "index": index, "source_path": source_path.clone() }),
                );
            entity_handles.push(handle);
        }

        self.loaded_scene = Some(FlecsSceneState {
            source_path: source_path.clone(),
            schema: schema.clone(),
            version,
            title: title.clone(),
            entity_count: entity_handles.len(),
            entity_handles: entity_handles.clone(),
        });

        FlecsSceneLoadResult {
            ok: true,
            source_path,
            replace,
            schema,
            version,
            title,
            entities: entity_handles,
            total_count: self.summary().entity_count,
        }
    }

    fn save_scene_value(&mut self) -> serde_json::Value {
        self.prune_dead_entities();
        let scene = self.loaded_scene.clone().unwrap_or_default();
        let loaded_entity_count = scene.entity_count;
        let loaded_entity_handles: Vec<serde_json::Value> = scene
            .entity_handles
            .iter()
            .map(|handle| serde_json::json!({ "stable_id": handle.stable_id }))
            .collect();
        let entities: Vec<serde_json::Value> = self
            .alive_entities
            .iter()
            .copied()
            .map(|stable_id| {
                serde_json::json!({
                    "handle": { "stable_id": stable_id },
                    "components": self.semantic_components.get(&stable_id).cloned().unwrap_or_default()
                })
            })
            .collect();
        serde_json::json!({
            "schema": scene.schema.unwrap_or_else(|| "newengine.scene.provider_snapshot.v1".to_owned()),
            "version": scene.version.unwrap_or(1),
            "title": scene.title,
            "source_path": scene.source_path,
            "authority": "shared-flecs-world",
            "loaded_entity_count": loaded_entity_count,
            "loaded_entity_handles": loaded_entity_handles,
            "entities": entities
        })
    }

    fn shutdown(&mut self) {
        let existing = std::mem::take(&mut self.alive_entities);
        for entity in existing {
            unsafe { ecs_delete(self.raw.as_ptr(), entity) };
        }
        self.tick = 0;
        self.entities_changed_tick = 0;
        self.semantic_components.clear();
        self.loaded_scene = None;
    }

    fn prune_dead_entities(&mut self) {
        let world = self.raw.as_ptr();
        self.alive_entities
            .retain(|entity| *entity != 0 && unsafe { ecs_is_alive(world, *entity) });
        self.semantic_components
            .retain(|entity, _| self.alive_entities.contains(entity));
    }
}

impl Drop for FlecsWorld {
    fn drop(&mut self) {
        let _ = unsafe { ecs_fini(self.raw.as_ptr()) };
    }
}

#[derive(Clone)]
struct FlecsEcsService {
    backend: Arc<Mutex<FlecsWorld>>,
    config: FlecsEcsPluginConfig,
}

impl FlecsEcsService {
    fn new(backend: Arc<Mutex<FlecsWorld>>, config: FlecsEcsPluginConfig) -> Self {
        Self { backend, config }
    }

    fn with_backend<T>(&self, f: impl FnOnce(&mut FlecsWorld) -> T) -> T {
        with_backend(&self.backend, f)
    }

    fn info(&self) -> EcsServiceInfo {
        EcsServiceInfo {
            protocol: "newengine.ecs-api/v1".to_owned(),
            features: vec![
                "gateway-summary".to_owned(),
                "entity-snapshot".to_owned(),
                "command-envelope".to_owned(),
                "semantic-component-packets".to_owned(),
                "flecs-world".to_owned(),
                "flecs-id-allocation".to_owned(),
                "shared-entity-authority".to_owned(),
                "single-source-of-truth".to_owned(),
            ],
            methods: ECS_REQUIRED_METHODS_V1.iter().map(|it| (*it).to_owned()).collect(),
        }
    }

    fn invoke_json(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EcsInvokeRequest>(payload, ECS_SERVICE_METHOD_INVOKE) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let payload = match serde_json::to_vec(&req.payload) {
            Ok(bytes) => Blob::from(bytes),
            Err(e) => return RResult::RErr(RString::from(e.to_string())),
        };

        match req.method.as_str() {
            ECS_SERVICE_METHOD_SUMMARY_JSON_V1 => self.summary_json_v1(),
            ECS_SERVICE_METHOD_SNAPSHOT_JSON_V1 => self.snapshot_json_v1(payload),
            ECS_SERVICE_METHOD_COMMAND_JSON_V1 => self.command_json_v1(payload),
            other => RResult::RErr(RString::from(format!(
                "flecs ecs service: invoke_json unknown target method '{other}'"
            ))),
        }
    }

    fn summary_json_v1(&self) -> RResult<Blob, RString> {
        let summary = self.with_backend(|backend| backend.summary());
        ok_json(&summary)
    }

    fn snapshot_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EcsSnapshotRequest>(payload, ECS_SERVICE_METHOD_SNAPSHOT_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let snapshot = self.with_backend(|backend| backend.snapshot(req));
        ok_json(&snapshot)
    }

    fn command_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EcsCommandRequest>(payload, ECS_SERVICE_METHOD_COMMAND_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let response = self.with_backend(|backend| backend.command(req));
        ok_json(&response)
    }

    fn shutdown_v1(&self) -> RResult<Blob, RString> {
        self.with_backend(|backend| backend.shutdown());
        RResult::ROk(Blob::from(Vec::new()))
    }
}

impl ServiceV1 for FlecsEcsService {
    fn id(&self) -> CapabilityId { CapabilityId::from(ECS_SERVICE_ID) }

    fn describe(&self) -> RString {
        let value = serde_json::json!({
            "id": ECS_SERVICE_ID,
            "gateway": ENGINE_ECS_SERVICE_ID,
            "version": 1,
            "backend_id": FLECS_BACKEND_ID,
            "backend_name": FLECS_ECS_PLUGIN_NAME,
            "backend_version": FLECS_ECS_PLUGIN_VERSION,
            "debug_text": self.config.debug_text,
            "methods": ECS_REQUIRED_METHODS_V1,
            "features": self.info().features,
            "authority": {
                "world": "shared-flecs-world",
                "entity_truth": true,
                "component_truth": "semantic-component-packets",
                "paired_gateway": ENGINE_ENTITY_SERVICE_ID,
                "paired_service": ENTITY_SERVICE_ID
            }
        });
        RString::from(serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_owned()))
    }

    fn call(&self, method: MethodName, payload: Blob) -> RResult<Blob, RString> {
        match method.as_str() {
            ECS_SERVICE_METHOD_INFO => ok_json(&self.info()),
            ECS_SERVICE_METHOD_INVOKE => self.invoke_json(payload),
            ECS_SERVICE_METHOD_SUMMARY_JSON_V1 => self.summary_json_v1(),
            ECS_SERVICE_METHOD_SNAPSHOT_JSON_V1 => self.snapshot_json_v1(payload),
            ECS_SERVICE_METHOD_COMMAND_JSON_V1 => self.command_json_v1(payload),
            ECS_SERVICE_METHOD_SHUTDOWN_V1 => self.shutdown_v1(),
            other => RResult::RErr(RString::from(format!("flecs ecs service: unknown method '{other}'"))),
        }
    }
}

#[derive(Clone)]
struct FlecsEntityService {
    backend: Arc<Mutex<FlecsWorld>>,
    config: FlecsEcsPluginConfig,
}

impl FlecsEntityService {
    fn new(backend: Arc<Mutex<FlecsWorld>>, config: FlecsEcsPluginConfig) -> Self {
        Self { backend, config }
    }

    fn with_backend<T>(&self, f: impl FnOnce(&mut FlecsWorld) -> T) -> T {
        with_backend(&self.backend, f)
    }

    fn info(&self) -> EntityServiceInfo {
        EntityServiceInfo {
            protocol: "newengine.entity-api/v1".to_owned(),
            features: vec![
                "opaque-stable-handles".to_owned(),
                "entity-list".to_owned(),
                "entity-exists".to_owned(),
                "entity-lifecycle".to_owned(),
                "flecs-world".to_owned(),
                "shared-ecs-authority".to_owned(),
                "single-source-of-truth".to_owned(),
            ],
            methods: ENTITY_REQUIRED_METHODS_V1.iter().map(|it| (*it).to_owned()).collect(),
        }
    }

    fn invoke_json(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EntityInvokeRequest>(payload, ENTITY_SERVICE_METHOD_INVOKE) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let payload = match serde_json::to_vec(&req.payload) {
            Ok(bytes) => Blob::from(bytes),
            Err(e) => return RResult::RErr(RString::from(e.to_string())),
        };

        match req.method.as_str() {
            ENTITY_SERVICE_METHOD_LIST_JSON_V1 => self.list_json_v1(payload),
            ENTITY_SERVICE_METHOD_EXISTS_JSON_V1 => self.exists_json_v1(payload),
            ENTITY_SERVICE_METHOD_SPAWN_JSON_V1 => self.spawn_json_v1(payload),
            ENTITY_SERVICE_METHOD_DESPAWN_JSON_V1 => self.despawn_json_v1(payload),
            other => RResult::RErr(RString::from(format!(
                "flecs entity service: invoke_json unknown target method '{other}'"
            ))),
        }
    }

    fn list_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EntityListRequest>(payload, ENTITY_SERVICE_METHOD_LIST_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let response = self.with_backend(|backend| backend.list_entities(req.limit));
        ok_json(&response)
    }

    fn exists_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EntityExistsRequest>(payload, ENTITY_SERVICE_METHOD_EXISTS_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let exists = self.with_backend(|backend| backend.exists_entity(req.entity));
        ok_json(&EntityExistsResponse { entity: req.entity, exists })
    }

    fn spawn_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EntitySpawnRequest>(payload, ENTITY_SERVICE_METHOD_SPAWN_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let response = self.with_backend(|backend| backend.spawn_entities(req.count));
        ok_json(&response)
    }

    fn despawn_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_payload::<EntityDespawnRequest>(payload, ENTITY_SERVICE_METHOD_DESPAWN_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let response = self.with_backend(|backend| backend.despawn_entities(req));
        ok_json(&response)
    }

    fn shutdown_v1(&self) -> RResult<Blob, RString> {
        self.with_backend(|backend| backend.shutdown());
        RResult::ROk(Blob::from(Vec::new()))
    }
}

impl ServiceV1 for FlecsEntityService {
    fn id(&self) -> CapabilityId { CapabilityId::from(ENTITY_SERVICE_ID) }

    fn describe(&self) -> RString {
        let value = serde_json::json!({
            "id": ENTITY_SERVICE_ID,
            "gateway": ENGINE_ENTITY_SERVICE_ID,
            "version": 1,
            "backend_id": FLECS_BACKEND_ID,
            "backend_name": FLECS_ECS_PLUGIN_NAME,
            "backend_version": FLECS_ECS_PLUGIN_VERSION,
            "debug_text": self.config.debug_text,
            "methods": ENTITY_REQUIRED_METHODS_V1,
            "features": self.info().features,
            "authority": {
                "world": "shared-flecs-world",
                "entity_truth": true,
                "component_truth": "semantic-component-packets",
                "paired_gateway": ENGINE_ECS_SERVICE_ID,
                "paired_service": ECS_SERVICE_ID
            }
        });
        RString::from(serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_owned()))
    }

    fn call(&self, method: MethodName, payload: Blob) -> RResult<Blob, RString> {
        match method.as_str() {
            ENTITY_SERVICE_METHOD_INFO => ok_json(&self.info()),
            ENTITY_SERVICE_METHOD_INVOKE => self.invoke_json(payload),
            ENTITY_SERVICE_METHOD_LIST_JSON_V1 => self.list_json_v1(payload),
            ENTITY_SERVICE_METHOD_EXISTS_JSON_V1 => self.exists_json_v1(payload),
            ENTITY_SERVICE_METHOD_SPAWN_JSON_V1 => self.spawn_json_v1(payload),
            ENTITY_SERVICE_METHOD_DESPAWN_JSON_V1 => self.despawn_json_v1(payload),
            ENTITY_SERVICE_METHOD_SHUTDOWN_V1 => self.shutdown_v1(),
            other => RResult::RErr(RString::from(format!("flecs entity service: unknown method '{other}'"))),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct FlecsSceneLoadResult {
    ok: bool,
    source_path: Option<String>,
    replace: bool,
    schema: Option<String>,
    version: Option<u64>,
    title: Option<String>,
    entities: Vec<EntityHandle>,
    total_count: u64,
}

#[derive(Clone)]
struct FlecsSceneService {
    backend: Arc<Mutex<FlecsWorld>>,
    host: HostApiV1,
    config: FlecsEcsPluginConfig,
}

impl FlecsSceneService {
    fn new(backend: Arc<Mutex<FlecsWorld>>, host: HostApiV1, config: FlecsEcsPluginConfig) -> Self {
        Self { backend, host, config }
    }

    fn with_backend<T>(&self, f: impl FnOnce(&mut FlecsWorld) -> T) -> T {
        with_backend(&self.backend, f)
    }

    fn formats_json(&self) -> RResult<Blob, RString> {
        ok_json(&serde_json::json!({
            "id": SCENE_SERVICE_ID,
            "gateway": ENGINE_SCENE_SERVICE_ID,
            "origin": "first-party-plugin",
            "owner": FLECS_ECS_PLUGIN_ID,
            "backend": FLECS_BACKEND_ID,
            "version": 1,
            "formats": [
                {
                    "id": "newengine.scene.provider-neutral.v1",
                    "schema": "json",
                    "media_type": "application/json",
                    "load": true,
                    "save": true
                }
            ],
            "authority": {
                "world": "shared-flecs-world",
                "scene_truth": true,
                "entity_truth": true,
                "component_truth": "semantic-component-packets"
            },
            "methods": SCENE_REQUIRED_METHODS
        }))
    }

    fn load_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_json_value(payload, scene_method::LOAD_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let replace = req.get("replace").and_then(serde_json::Value::as_bool).unwrap_or(true);
        if !replace {
            return RResult::RErr(RString::from("flecs scene service load_json_v1 supports replace=true only"));
        }

        let path = req
            .get("path")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|it| !it.is_empty())
            .map(str::to_owned);

        let scene_value = if let Some(inline) = req.get("scene").or_else(|| req.get("payload")).or_else(|| req.get("asset")) {
            inline.clone()
        } else if let Some(path) = path.as_ref() {
            let assets = AssetServiceClient::new(self.host.clone());
            let bytes = match assets.text_v1(path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    return RResult::RErr(RString::from(format!(
                        "flecs scene service cannot read scene asset path='{path}' err='{e}'"
                    )));
                }
            };
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(value) => value,
                Err(e) => {
                    return RResult::RErr(RString::from(format!(
                        "flecs scene service cannot parse scene asset path='{path}' err='{e}'"
                    )));
                }
            }
        } else {
            return RResult::RErr(RString::from(
                "flecs scene service load_json_v1 requires path, scene, payload or asset",
            ));
        };

        let result = self.with_backend(|backend| backend.load_scene_value(scene_value, path, replace));
        ok_json(&result)
    }

    fn save_json_v1(&self, payload: Blob) -> RResult<Blob, RString> {
        let req = match decode_json_value(payload, scene_method::SAVE_JSON_V1) {
            Ok(v) => v,
            Err(e) => return RResult::RErr(e),
        };
        let path = req.get("path").and_then(serde_json::Value::as_str).unwrap_or("");
        let pretty = req.get("pretty").and_then(serde_json::Value::as_bool).unwrap_or(true);
        let payload = self.with_backend(|backend| backend.save_scene_value());
        let payload_text = if pretty {
            serde_json::to_string_pretty(&payload)
        } else {
            serde_json::to_string(&payload)
        }
        .unwrap_or_else(|_| "{}".to_owned());
        ok_json(&serde_json::json!({
            "ok": true,
            "path": path,
            "stored": false,
            "storage": "caller-owned",
            "authority": "shared-flecs-world",
            "pretty": pretty,
            "payload": payload,
            "payload_text": payload_text
        }))
    }

    fn shutdown_v1(&self) -> RResult<Blob, RString> {
        RResult::ROk(Blob::from(Vec::new()))
    }
}

impl ServiceV1 for FlecsSceneService {
    fn id(&self) -> CapabilityId { CapabilityId::from(SCENE_SERVICE_ID) }

    fn describe(&self) -> RString {
        let value = serde_json::json!({
            "id": SCENE_SERVICE_ID,
            "gateway": ENGINE_SCENE_SERVICE_ID,
            "version": 1,
            "backend_id": FLECS_BACKEND_ID,
            "backend_name": FLECS_ECS_PLUGIN_NAME,
            "backend_version": FLECS_ECS_PLUGIN_VERSION,
            "debug_text": self.config.debug_text,
            "methods": SCENE_REQUIRED_METHODS,
            "features": [
                "scene-load-save",
                "scene-authority",
                "semantic-component-packets",
                "single-source-of-truth"
            ],
            "authority": {
                "world": "shared-flecs-world",
                "scene_truth": true,
                "entity_truth": true,
                "component_truth": "semantic-component-packets",
                "paired_gateway": ENGINE_ECS_SERVICE_ID,
                "paired_entity_gateway": ENGINE_ENTITY_SERVICE_ID
            }
        });
        RString::from(serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_owned()))
    }

    fn call(&self, method: MethodName, payload: Blob) -> RResult<Blob, RString> {
        match method.as_str() {
            scene_method::FORMATS_JSON => self.formats_json(),
            scene_method::LOAD_JSON_V1 => self.load_json_v1(payload),
            scene_method::SAVE_JSON_V1 => self.save_json_v1(payload),
            scene_method::SHUTDOWN_V1 => self.shutdown_v1(),
            other => RResult::RErr(RString::from(format!("flecs scene service: unknown method '{other}'"))),
        }
    }
}

fn normalize_scene_payload(value: serde_json::Value) -> serde_json::Value {
    if let Some(scene) = value.get("scene") {
        scene.clone()
    } else if let Some(payload) = value.get("payload") {
        payload.clone()
    } else {
        value
    }
}

fn extract_scene_entity_records(scene: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(entities) = scene.get("entities").and_then(serde_json::Value::as_array) {
        return entities.clone();
    }

    let mut records = Vec::new();
    for key in ["player", "terrain", "sky", "lighting", "foliage", "audio", "postfx"] {
        if let Some(value) = scene.get(key) {
            records.push(serde_json::json!({
                "kind": format!("scene.{}", key),
                "name": key,
                "payload": value
            }));
        }
    }

    if let Some(prefabs) = scene.get("prefabs").and_then(serde_json::Value::as_array) {
        for (index, prefab) in prefabs.iter().enumerate() {
            records.push(serde_json::json!({
                "kind": "scene.prefab",
                "name": format!("prefab_{}", index),
                "payload": prefab
            }));
        }
    }

    records
}

fn with_backend<T>(backend: &Arc<Mutex<FlecsWorld>>, f: impl FnOnce(&mut FlecsWorld) -> T) -> T {
    let mut guard = match backend.lock() {
        Ok(v) => v,
        Err(e) => e.into_inner(),
    };
    f(&mut guard)
}

fn ok_json<T: serde::Serialize>(value: &T) -> RResult<Blob, RString> {
    match serde_json::to_vec(value) {
        Ok(bytes) => RResult::ROk(Blob::from(bytes)),
        Err(e) => RResult::RErr(RString::from(e.to_string())),
    }
}

fn decode_json_value(payload: Blob, method: &str) -> Result<serde_json::Value, RString> {
    if payload.is_empty() {
        Ok(serde_json::json!({}))
    } else {
        serde_json::from_slice(payload.as_slice()).map_err(|e| RString::from(format!("{method}: {e}")))
    }
}

fn decode_payload<T: serde::de::DeserializeOwned>(payload: Blob, method: &str) -> Result<T, RString> {
    if payload.is_empty() {
        serde_json::from_slice(b"{}").map_err(|e| RString::from(format!("{method}: {e}")))
    } else {
        serde_json::from_slice(payload.as_slice()).map_err(|e| RString::from(format!("{method}: {e}")))
    }
}

fn parse_backend_config_blob(blob: &ConfigBlobV1) -> Result<FlecsEcsPluginConfig, String> {
    if blob.bytes.is_empty() {
        return Ok(FlecsEcsPluginConfig::default());
    }
    let parsed = parse_json_object(blob.bytes.as_slice(), "flecs ecs config")?;
    parse_backend_config_value(&parsed)
}

fn parse_backend_config_value(value: &serde_json::Value) -> Result<FlecsEcsPluginConfig, String> {
    let mut out = FlecsEcsPluginConfig::default();
    if let Some(debug_text) = value.get("debug_text").and_then(serde_json::Value::as_str) {
        out.debug_text = debug_text.to_owned();
    }
    if let Some(v) = value.get("initial_entity_capacity").and_then(serde_json::Value::as_i64) {
        out.initial_entity_capacity = v.clamp(0, i64::from(i32::MAX)) as i32;
    }
    if let Some(v) = value.get("minimal_world").and_then(serde_json::Value::as_bool) {
        out.minimal_world = v;
    }
    if let Some(v) = value.get("progress_on_advance_tick").and_then(serde_json::Value::as_bool) {
        out.progress_on_advance_tick = v;
    }
    Ok(out)
}

fn parse_json_object(raw: &[u8], what: &str) -> Result<serde_json::Value, String> {
    let parsed: serde_json::Value = serde_json::from_slice(raw)
        .map_err(|e| format!("{what} parse failed: {e}"))?;
    if parsed.is_object() { Ok(parsed) } else { Err(format!("{what} must be a JSON object")) }
}

fn merge_json_replace(dst: &mut serde_json::Value, src: &serde_json::Value) {
    match (dst, src) {
        (serde_json::Value::Object(dst_map), serde_json::Value::Object(src_map)) => {
            for (k, v) in src_map {
                merge_json_replace(dst_map.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (dst_slot, src_value) => *dst_slot = src_value.clone(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn newengine_plugin_signature_v1() -> PluginSignatureV1 {
    PluginSignatureV1 {
        id: RString::from(FLECS_ECS_PLUGIN_ID),
        name: RString::from(FLECS_ECS_PLUGIN_NAME),
        version: RString::from(FLECS_ECS_PLUGIN_VERSION),
        kind: PluginKind::Runtime,
        bootstrap_phase: PluginBootstrapPhase::Engine,
    }
}

export_newengine_plugin!(module = FlecsEcsPlugin::default());

#[cfg(test)]
mod tests {
    use super::*;

    fn test_backend() -> Arc<Mutex<FlecsWorld>> {
        Arc::new(Mutex::new(FlecsWorld::new(&FlecsEcsPluginConfig::default()).unwrap()))
    }

    #[test]
    fn descriptor_declares_engine_ecs_and_entity_routes() {
        let descriptor = FlecsEcsPlugin::descriptor();
        assert_eq!(descriptor.id.as_str(), FLECS_ECS_PLUGIN_ID);
        assert!(descriptor.capabilities.iter().any(|cap| {
            cap.id.as_str() == ECS_SERVICE_ID
                && cap.role == CapabilityRole::Provides
                && cap.kind == CapabilityKind::ServiceV1
        }));
        assert!(descriptor.capabilities.iter().any(|cap| {
            cap.id.as_str() == ENTITY_SERVICE_ID
                && cap.role == CapabilityRole::Provides
                && cap.kind == CapabilityKind::ServiceV1
        }));
        assert!(descriptor.capabilities.iter().any(|cap| {
            cap.id.as_str() == SCENE_SERVICE_ID
                && cap.role == CapabilityRole::Provides
                && cap.kind == CapabilityKind::ServiceV1
        }));

        let ecs_backend = descriptor
            .capabilities
            .iter()
            .find(|cap| cap.id.as_str() == ECS_BACKEND_CAPABILITY_ID)
            .expect("ecs.backend capability");
        let ecs_json: serde_json::Value = serde_json::from_str(ecs_backend.describe_json.as_str()).unwrap();
        assert_eq!(ecs_json["service_kind"], "ecs");
        assert_eq!(ecs_json["engine_gateway"], ENGINE_ECS_SERVICE_ID);
        assert_eq!(ecs_json["provider_route"], ECS_PROVIDER_GATEWAY_ID);
        assert_eq!(ecs_json["system_tags"][0], "provider.implementation_route");
        assert_eq!(ecs_json["contract"], ECS_SERVICE_ID);
        assert_eq!(ecs_json["backend"], FLECS_BACKEND_ID);

        let entity_backend = descriptor
            .capabilities
            .iter()
            .find(|cap| cap.id.as_str() == ENTITY_BACKEND_CAPABILITY_ID)
            .expect("entity.backend capability");
        let entity_json: serde_json::Value = serde_json::from_str(entity_backend.describe_json.as_str()).unwrap();
        assert_eq!(entity_json["service_kind"], "entity");
        assert_eq!(entity_json["engine_gateway"], ENGINE_ENTITY_SERVICE_ID);
        assert_eq!(entity_json["provider_route"], ENTITY_PROVIDER_GATEWAY_ID);
        assert_eq!(entity_json["system_tags"][0], "provider.implementation_route");
        assert_eq!(entity_json["contract"], ENTITY_SERVICE_ID);
        assert_eq!(entity_json["backend"], FLECS_BACKEND_ID);

        let scene_backend = descriptor
            .capabilities
            .iter()
            .find(|cap| cap.id.as_str() == SCENE_BACKEND_CAPABILITY_ID)
            .expect("scene.backend capability");
        let scene_json: serde_json::Value = serde_json::from_str(scene_backend.describe_json.as_str()).unwrap();
        assert_eq!(scene_json["service_kind"], "scene");
        assert_eq!(scene_json["engine_gateway"], ENGINE_SCENE_SERVICE_ID);
        assert_eq!(scene_json["provider_route"], SCENE_PROVIDER_GATEWAY_ID);
        assert_eq!(scene_json["system_tags"][0], "provider.implementation_route");
        assert_eq!(scene_json["contract"], SCENE_SERVICE_ID);
        assert_eq!(scene_json["backend"], FLECS_BACKEND_ID);
    }

    #[test]
    fn ecs_spawn_is_visible_through_entity_gateway() {
        let backend = test_backend();
        let ecs = FlecsEcsService::new(backend.clone(), FlecsEcsPluginConfig::default());
        let entity = FlecsEntityService::new(backend, FlecsEcsPluginConfig::default());

        let req = EcsCommandRequest { commands: vec![EcsCommand::SpawnEmpty, EcsCommand::AdvanceTick] };
        let response_blob = ecs.command_json_v1(Blob::from(serde_json::to_vec(&req).unwrap())).into_result().unwrap();
        let response: EcsCommandResponse = serde_json::from_slice(response_blob.as_slice()).unwrap();
        assert!(response.ok);
        assert_eq!(response.summary.entity_count, 1);
        assert_eq!(response.summary.tick, 1);

        let list_blob = entity
            .list_json_v1(Blob::from(serde_json::to_vec(&EntityListRequest::default()).unwrap()))
            .into_result()
            .unwrap();
        let list: EntityListResponse = serde_json::from_slice(list_blob.as_slice()).unwrap();
        assert_eq!(list.total_count, 1);
        assert_eq!(list.entities.len(), 1);
    }

    #[test]
    fn scene_load_creates_authoritative_flecs_entities() {
        let backend = test_backend();
        let ecs = FlecsEcsService::new(backend.clone(), FlecsEcsPluginConfig::default());

        let scene_value = serde_json::json!({
            "schema": "test.scene",
            "version": 1,
            "entities": [
                { "name": "A", "transform": { "position": [0, 0, 0] } },
                { "name": "B", "transform": { "position": [1, 0, 0] } }
            ]
        });
        let load = with_backend(&backend, |world| world.load_scene_value(scene_value, None, true));
        assert_eq!(load.entities.len(), 2);

        let summary_blob = ecs.summary_json_v1().into_result().unwrap();
        let summary: EcsWorldSummary = serde_json::from_slice(summary_blob.as_slice()).unwrap();
        assert_eq!(summary.entity_count, 2);
        assert!(summary.storage_count >= 1);
    }

    #[test]
    fn entity_spawn_and_despawn_are_visible_through_ecs_snapshot() {
        let backend = test_backend();
        let ecs = FlecsEcsService::new(backend.clone(), FlecsEcsPluginConfig::default());
        let entity = FlecsEntityService::new(backend, FlecsEcsPluginConfig::default());

        let spawn_blob = entity
            .spawn_json_v1(Blob::from(serde_json::to_vec(&EntitySpawnRequest { count: 2 }).unwrap()))
            .into_result()
            .unwrap();
        let spawn: EntitySpawnResponse = serde_json::from_slice(spawn_blob.as_slice()).unwrap();
        assert_eq!(spawn.entities.len(), 2);
        assert_eq!(spawn.total_count, 2);

        let snapshot_blob = ecs
            .snapshot_json_v1(Blob::from(serde_json::to_vec(&EcsSnapshotRequest::default()).unwrap()))
            .into_result()
            .unwrap();
        let snapshot: EcsWorldSnapshot = serde_json::from_slice(snapshot_blob.as_slice()).unwrap();
        assert_eq!(snapshot.summary.entity_count, 2);
        assert_eq!(snapshot.entities.len(), 2);

        let despawn_req = EntityDespawnRequest { entities: vec![spawn.entities[0].handle] };
        let despawn_blob = entity
            .despawn_json_v1(Blob::from(serde_json::to_vec(&despawn_req).unwrap()))
            .into_result()
            .unwrap();
        let despawn: EntityDespawnResponse = serde_json::from_slice(despawn_blob.as_slice()).unwrap();
        assert!(despawn.ok);
        assert_eq!(despawn.total_count, 1);

        let summary_blob = ecs.summary_json_v1().into_result().unwrap();
        let summary: EcsWorldSummary = serde_json::from_slice(summary_blob.as_slice()).unwrap();
        assert_eq!(summary.entity_count, 1);
    }
}
