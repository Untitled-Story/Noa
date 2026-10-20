//! Resource and material converters. Shader identity is verified by program
//! fingerprint, never by a material name or stale saved blend properties.
use super::{EntityIds, UnitySources, wire_util::*};
use crate::{diagnostics::Error, resolver::AssetIndex, unity::Guid};
use noa_format::{
    ir::{EntityId, ReferenceKind, TypedReference},
    wire as w,
};
use serde_json::json;
use serde_yaml_ng::Value as Y;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs};

mod builtin_shaders;

pub(crate) struct Resources<'a> {
    pub index: &'a AssetIndex,
    pub sources: &'a UnitySources,
    pub ids: &'a EntityIds,
    pub next: u32,
    pub materials: BTreeMap<u32, w::MaterialT>,
    pub assets: BTreeMap<u32, w::AssetT>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub targets: BTreeMap<(String, i64), TypedReference>,
}
impl<'a> Resources<'a> {
    pub fn new(
        index: &'a AssetIndex,
        sources: &'a UnitySources,
        ids: &'a EntityIds,
        next: u32,
    ) -> Self {
        Self {
            index,
            sources,
            ids,
            next,
            materials: BTreeMap::new(),
            assets: BTreeMap::new(),
            files: BTreeMap::new(),
            targets: BTreeMap::new(),
        }
    }
    pub fn allocate(&mut self) -> Result<u32, Error> {
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| err("IDs", "exhausted"))?;
        Ok(id)
    }
    pub fn reference(&self, entry: &str, r: &Y) -> Result<(String, i64), Error> {
        let fid = int(r, "fileID")?;
        let source = match r.get("guid").and_then(Y::as_str) {
            None => entry.into(),
            Some(g) => {
                let guid: Guid = g.parse()?;
                if guid.is_zero() {
                    entry.into()
                } else if guid.is_builtin() {
                    format!("builtin/{guid}")
                } else {
                    self.sources
                        .guid_sources
                        .get(&guid)
                        .cloned()
                        .or_else(|| {
                            self.index
                                .assets
                                .get(&guid)
                                .map(|a| self.index.label(&a.path))
                        })
                        .ok_or_else(|| err("reference", format!("missing {guid}")))?
                }
            }
        };
        Ok((source, fid))
    }
    fn target(&mut self, key: (String, i64), id: u32, kind: ReferenceKind) {
        self.targets.insert(
            key,
            TypedReference {
                kind,
                id: EntityId::new(id).unwrap(),
            },
        );
    }
    fn asset(&mut self, id: u32, kind: w::AssetKind, extension: &str, bytes: Vec<u8>) {
        self.assets.insert(id, n!(AssetT { id, kind }));
        self.files.insert(format!("assets/{id}.{extension}"), bytes);
    }
    pub fn material(&mut self, entry: &str, r: &Y) -> Result<u32, Error> {
        if int(r, "fileID")? == 0 {
            return Ok(0);
        }
        let key = self.reference(entry, r)?;
        if let Some(t) = self.targets.get(&key) {
            return Ok(t.id.get());
        }
        if key.0.starts_with("builtin/") {
            return self.default_material(key);
        }
        let o = self
            .sources
            .files
            .get(&key.0)
            .and_then(|f| f.objects.get(&key.1))
            .ok_or_else(|| err("Material", "not found"))?;
        if o.class_id != 21 {
            return Err(err("Material", "wrong source class"));
        }
        let v = o.data.clone();
        let id = self.ids.get(&key.0, key.1)?.get();
        let shader_key = self.reference(&key.0, get(&v, "m_Shader")?)?;
        if shader_key.0.starts_with("builtin/") {
            return self.legacy_particle_material(key, id, &v, shader_key);
        }
        let shader = self
            .sources
            .files
            .get(&shader_key.0)
            .and_then(|f| f.objects.get(&shader_key.1))
            .ok_or_else(|| err("Shader", "not found"))?;
        let blob = shader
            .data
            .get("compressedBlob")
            .and_then(Y::as_str)
            .ok_or_else(|| err("Shader", "expected compiled program"))?;
        let bytes = hex(blob)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if hash != "f4c3959fbb391fd5e4722820c271a04f3d3ced65bd2029429033936e7ae264f9" {
            return Err(err("Shader", format!("unrecognized program {hash}")));
        }
        let sid = self.ids.get(&shader_key.0, shader_key.1)?.get();
        let shader_data = json!({"format":"noa-shader-data","version":1,"model":"particle-add-alpha","program_sha256":hash,"revision":1,"color_formula":"c = texture(MainTex, uv) * vertexColor; rgb = c.rgb * c.a; alpha = custom1.x <= 0.5 ? c.a : 0","output_policy":"primary-color","outputs":[0],"source_outputs":[0,1,2],"alpha_mode":{"stream":0,"component":0,"comparison":"less_equal","threshold":0.5},"rgb_premultiply":true,"source_passes":shader.data["m_ParsedForm"]["m_SubShaders"],"source_program_encoding":"unity-metal-lz4","source_program_hex":blob});
        self.asset(
            sid,
            w::AssetKind::ShaderData,
            "shader.json",
            serde_json::to_vec(&shader_data).map_err(|e| err("shader data", e))?,
        );
        self.target(shader_key, sid, ReferenceKind::Asset);
        let parsed = get(&shader.data, "m_ParsedForm")?;
        let subs = get(parsed, "m_SubShaders")?
            .as_sequence()
            .ok_or_else(|| err("shader", "expected subshaders"))?;
        let first = subs.first().ok_or_else(|| err("shader", "no subshader"))?;
        let pass = get(first, "m_Passes")?
            .as_sequence()
            .and_then(|v| v.first())
            .ok_or_else(|| err("shader", "no pass"))?;
        let state = get(pass, "m_State")?;
        let saved = get(&v, "m_SavedProperties")?;
        let mut textures = Vec::new();
        // This verified program consumes only _MainTex; inactive saved texture slots
        // are editor leftovers, not dependencies of this program.
        let binding = get(get(saved, "m_TexEnvs")?, "_MainTex")?;
        let (image_id, space, sampler) = self.image(&key.0, get(binding, "m_Texture")?)?;
        textures.push(n!(TextureBindingT {
            slot: "_MainTex".into(),
            image_id,
            scale: Some(vec2(get(binding, "m_Scale")?)?),
            offset: Some(vec2(get(binding, "m_Offset")?)?),
            color_space: space,
            sampler: Some(Box::new(sampler))
        }));
        let queue = int(&v, "m_CustomRenderQueue")?;
        let m = n!(MaterialT {
            id,
            name: v.get("m_Name").and_then(Y::as_str).map(str::to_owned),
            model: w::MaterialModel::ParticleAddAlpha,
            tint: Some(n!(ColorT {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 1.
            })),
            textures: Some(textures),
            render_state: Box::new(render_state(state)?),
            render_queue: if queue == -1 {
                3000
            } else {
                i32::try_from(queue).map_err(|_| err("queue", "outside int32"))?
            },
            source_shader: shader
                .data
                .get("m_Name")
                .and_then(Y::as_str)
                .map(str::to_owned),
            shader_data_asset_id: sid
        });
        self.materials.insert(id, m);
        self.target(key, id, ReferenceKind::Material);
        Ok(id)
    }
    pub(crate) fn image(
        &mut self,
        entry: &str,
        r: &Y,
    ) -> Result<(u32, w::ColorSpace, w::SamplerT), Error> {
        let key = self.reference(entry, r)?;
        if key.1 != 2800000 {
            return Err(err("Texture", "only Texture2D fileID supported"));
        }
        let asset = self
            .index
            .assets
            .values()
            .find(|a| self.index.label(&a.path) == key.0)
            .ok_or_else(|| err("Texture", "missing source image"))?;
        if asset.path.extension().and_then(|v| v.to_str()) != Some("png") {
            return Err(err("Texture", "only PNG input supported"));
        }
        let bytes = fs::read(&asset.path).map_err(|e| Error::io(&asset.path, e))?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(err("Texture", "invalid PNG"));
        }
        let meta_path = asset.path.with_extension("png.meta");
        let text = fs::read_to_string(&meta_path).map_err(|e| Error::io(&meta_path, e))?;
        let meta: Y = serde_yaml_ng::from_str(&text).map_err(|e| err("texture metadata", e))?;
        let importer = get(&meta, "TextureImporter")?;
        let space = if flag(get(importer, "mipmaps")?, "sRGBTexture")? {
            w::ColorSpace::SRGB
        } else {
            w::ColorSpace::Linear
        };
        let settings = get(importer, "textureSettings")?;
        let filter = enum_value(
            int(settings, "filterMode")?,
            &[
                w::FilterMode::Nearest,
                w::FilterMode::Linear,
                w::FilterMode::Trilinear,
            ],
            "texture filter",
        )?;
        let wrap = |k| {
            enum_value(
                int(settings, k)?,
                &[
                    w::AddressMode::Repeat,
                    w::AddressMode::Clamp,
                    w::AddressMode::Mirror,
                ],
                "texture wrap",
            )
        };
        let sampler = n!(SamplerT {
            min_filter: filter,
            mag_filter: if filter == w::FilterMode::Nearest {
                filter
            } else {
                w::FilterMode::Linear
            },
            wrap_u: wrap("wrapU")?,
            wrap_v: wrap("wrapV")?
        });
        let id = if let Some(t) = self.targets.get(&key) {
            t.id.get()
        } else {
            let id = self.allocate()?;
            self.asset(id, w::AssetKind::Image, "png", bytes);
            self.target(key, id, ReferenceKind::Asset);
            id
        };
        Ok((id, space, sampler))
    }
    fn default_material(&mut self, key: (String, i64)) -> Result<u32, Error> {
        if key != ("builtin/0000000000000000f000000000000000".into(), 10308) {
            return Err(err("builtin material", format!("unknown {key:?}")));
        }
        let image = self.allocate()?;
        self.asset(
            image,
            w::AssetKind::Image,
            "png",
            include_bytes!("../../resources/default-particle.png").to_vec(),
        );
        let id = self.allocate()?;
        let state = n!(RenderStateT {
            src_rgb: w::BlendFactor::SrcAlpha,
            dst_rgb: w::BlendFactor::OneMinusSrcAlpha,
            src_alpha: w::BlendFactor::SrcAlpha,
            dst_alpha: w::BlendFactor::OneMinusSrcAlpha,
            depth_test: w::CompareOp::LessEqual,
            cull: w::CullMode::Back
        });
        self.materials.insert(
            id,
            n!(MaterialT {
                id,
                name: Some("Default-ParticleSystem".into()),
                model: w::MaterialModel::ParticleUnlit,
                tint: Some(n!(ColorT {
                    r: 1.,
                    g: 1.,
                    b: 1.,
                    a: 1.
                })),
                textures: Some(vec![n!(TextureBindingT {
                    slot: "_MainTex".into(),
                    image_id: image,
                    scale: Some(n!(Vec2T { x: 1., y: 1. })),
                    offset: Some(w::Vec2T::default()),
                    color_space: w::ColorSpace::SRGB,
                    sampler: Some(Box::new(n!(SamplerT {
                        wrap_u: w::AddressMode::Repeat,
                        wrap_v: w::AddressMode::Repeat
                    })))
                })]),
                render_state: Box::new(state),
                source_shader: Some("Particles/Standard Unlit".into())
            }),
        );
        self.target(key, id, ReferenceKind::Material);
        Ok(id)
    }
}
fn hex(s: &str) -> Result<Vec<u8>, Error> {
    if !s.is_ascii() || !s.len().is_multiple_of(2) {
        return Err(err("hex", "odd length"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| err("hex", "invalid byte")))
        .collect()
}
fn render_state(s: &Y) -> Result<w::RenderStateT, Error> {
    let val = |v: &Y, k: &str| -> Result<i64, Error> {
        let f = get(v, k)?;
        if f.get("name")
            .and_then(Y::as_str)
            .is_some_and(|n| n != "<noninit>" && !n.is_empty())
        {
            return Err(err("render state", "dynamic property binding unsupported"));
        }
        int(f, "val")
    };
    let b = get(s, "rtBlend0")?;
    let factors = [
        w::BlendFactor::Zero,
        w::BlendFactor::One,
        w::BlendFactor::DstColor,
        w::BlendFactor::SrcColor,
        w::BlendFactor::OneMinusDstColor,
        w::BlendFactor::SrcAlpha,
        w::BlendFactor::OneMinusSrcColor,
        w::BlendFactor::DstAlpha,
        w::BlendFactor::OneMinusDstAlpha,
        w::BlendFactor::SrcAlpha,
        w::BlendFactor::OneMinusSrcAlpha,
    ];
    let factor = |k| {
        let v = val(b, k)?;
        if v == 9 {
            return Err(err("blend", "SrcAlphaSaturate unsupported"));
        }
        enum_value(v, &factors, k)
    };
    let ops = [
        w::BlendOp::Add,
        w::BlendOp::Subtract,
        w::BlendOp::ReverseSubtract,
        w::BlendOp::Min,
        w::BlendOp::Max,
    ];
    let depth = enum_value(
        val(s, "zTest")?,
        &[
            w::CompareOp::Always,
            w::CompareOp::Never,
            w::CompareOp::Less,
            w::CompareOp::Equal,
            w::CompareOp::LessEqual,
            w::CompareOp::Greater,
            w::CompareOp::NotEqual,
            w::CompareOp::GreaterEqual,
            w::CompareOp::Always,
        ],
        "zTest",
    )?;
    let mask = u8::try_from(val(b, "colMask")?).map_err(|_| err("color mask", "invalid"))?;
    // Unity mask bits R=8,G=4,B=2,A=1 -> NOIR bits R=1,G=2,B=4,A=8.
    let color_write_mask =
        ((mask & 8) >> 3) | ((mask & 4) >> 1) | ((mask & 2) << 1) | ((mask & 1) << 3);
    Ok(n!(RenderStateT {
        src_rgb: factor("srcBlend")?,
        dst_rgb: factor("destBlend")?,
        src_alpha: factor("srcBlendAlpha")?,
        dst_alpha: factor("destBlendAlpha")?,
        rgb_op: enum_value(val(b, "blendOp")?, &ops, "blend op")?,
        alpha_op: enum_value(val(b, "blendOpAlpha")?, &ops, "alpha blend op")?,
        depth_test: depth,
        depth_write: val(s, "zWrite")? != 0,
        cull: enum_value(
            val(s, "culling")?,
            &[w::CullMode::None, w::CullMode::Front, w::CullMode::Back],
            "cull"
        )?,
        color_write_mask
    }))
}
