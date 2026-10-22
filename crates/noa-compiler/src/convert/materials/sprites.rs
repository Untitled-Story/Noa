//! Sprite resources: independent image, real triangle geometry and authored metadata.
use super::*;
use noa_format::mesh::TexCoord;
impl Resources<'_> {
    pub(crate) fn sprite(&mut self, entry: &str, r: &Y) -> Result<u32, Error> {
        let key = self.reference(entry, r)?;
        if let Some(t) = self.targets.get(&key) {
            return Ok(t.id.get());
        }
        let object = self
            .sources
            .files
            .get(&key.0)
            .and_then(|f| f.objects.get(&key.1))
            .ok_or_else(|| err("Sprite", "not found"))?;
        if object.class_id != 213 {
            return Err(err("Sprite", "expected serialized Sprite asset"));
        }
        let v = object.data.clone();
        let rd = get(&v, "m_RD")?;
        if int(get(&v, "m_SpriteAtlas")?, "fileID")? != 0 {
            return Err(err(
                "Sprite",
                "atlas binding must be resolved during export",
            ));
        }
        if int(get(rd, "alphaTexture")?, "fileID")? != 0 {
            return Err(err("Sprite", "separate alpha texture unsupported"));
        }
        if rd
            .get("secondaryTextures")
            .and_then(Y::as_sequence)
            .is_some_and(|a| !a.is_empty())
        {
            return Err(err(
                "Sprite",
                "secondary textures need a defined material recipe",
            ));
        }
        let (image_id, space, sampler) = self.image(&key.0, get(rd, "texture")?)?;
        let image = &self.files[&format!("assets/{image_id}.png")];
        let width = u32::from_be_bytes(
            image
                .get(16..20)
                .ok_or_else(|| err("Sprite", "PNG dimensions absent"))?
                .try_into()
                .unwrap(),
        ) as f32;
        let height = u32::from_be_bytes(image[20..24].try_into().unwrap()) as f32;
        if width == 0. || height == 0. {
            return Err(err("Sprite", "invalid texture size"));
        }
        let mut mesh = crate::assets::geometry::decode(rd, v["m_Name"].as_str().unwrap_or(""), 0)?;
        let flags = uint(rd, "settingsRaw")?;
        let rotation = (flags >> 2) & 15;
        if rotation > 4 {
            return Err(err("Sprite", "unknown packing rotation"));
        }
        let uv = get(rd, "uvTransform")?;
        let tr = rect(get(rd, "textureRect")?)?;
        // AssetRipper exports native Sprite position streams but may leave its UV stream
        // zero. Reconstruct from the serialized pixel affine transform, never a full Quad.
        if mesh
            .texcoords
            .iter()
            .find(|u| u.channel == 0)
            .is_none_or(|u| u.values.iter().all(|v| v[0] == 0. && v[1] == 0.))
        {
            if flags & 1 != 0 {
                return Err(err(
                    "Sprite",
                    "packed Sprite with missing UV stream cannot be reconstructed reliably",
                ));
            }
            let mut values = Vec::with_capacity(mesh.positions.len());
            for p in &mesh.positions {
                let x = p.x * num(uv, "x")? + num(uv, "y")?;
                let y = p.y * num(uv, "z")? + num(uv, "w")?;
                values.push([x / width, y / height, 0., 0.]);
            }
            mesh.texcoords.retain(|u| u.channel != 0);
            mesh.texcoords.push(TexCoord {
                channel: 0,
                dimension: 2,
                values,
            });
        }
        let mesh_id = self.allocate()?;
        self.asset(
            mesh_id,
            w::AssetKind::Mesh,
            "noms",
            mesh.encode().map_err(|e| err("Sprite", e))?,
        );
        let id = self.ids.get(&key.0, key.1)?.get();
        let descriptor = n!(SpriteAssetT {
            mesh_asset_id: mesh_id,
            image_asset_id: image_id,
            rect: Some(rect(get(&v, "m_Rect")?)?),
            texture_rect: Some(tr),
            texture_rect_offset: Some(vec2(get(rd, "textureRectOffset")?)?),
            atlas_rect_offset: Some(vec2(get(rd, "atlasRectOffset")?)?),
            pivot: Some(vec2(get(&v, "m_Pivot")?)?),
            border: Some(vec4(get(&v, "m_Border")?)?),
            pixels_per_unit: num(&v, "m_PixelsToUnits")?,
            uv_transform: Some(vec4(uv)?),
            packing_flags: flags,
            downscale_multiplier: num(rd, "downscaleMultiplier")?,
            sampler: Some(Box::new(sampler)),
            color_space: space
        });
        self.assets.insert(
            id,
            n!(AssetT {
                id,
                kind: w::AssetKind::Sprite,
                sprite: Some(Box::new(descriptor))
            }),
        );
        self.target(key, id, ReferenceKind::Asset);
        Ok(id)
    }
}
fn rect(v: &Y) -> Result<w::Vec4T, Error> {
    Ok(n!(Vec4T {
        x: num(v, "x")?,
        y: num(v, "y")?,
        z: num(v, "width")?,
        w: num(v, "height")?
    }))
}
fn vec4(v: &Y) -> Result<w::Vec4T, Error> {
    Ok(n!(Vec4T {
        x: num(v, "x")?,
        y: num(v, "y")?,
        z: num(v, "z")?,
        w: num(v, "w")?
    }))
}
