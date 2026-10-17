"""Check published shader-data contracts; jsonschema is a development dependency."""
import copy
import json
from pathlib import Path
import unittest
try:
    import jsonschema
except ImportError:
    jsonschema = None

@unittest.skipIf(jsonschema is None, "install jsonschema to validate published recipe contracts")
class ShaderRecipeTests(unittest.TestCase):
    def setUp(self):
        schema = json.loads((Path(__file__).parents[2] / "schemas/shader-data.schema.json").read_text())
        jsonschema.Draft202012Validator.check_schema(schema)
        self.validator = jsonschema.Draft202012Validator(schema)

    def recipe(self):
        return {
            "format": "noa-shader-data", "version": 1, "revision": 1,
            "model": "unity-legacy-particle-additive",
            "source_builtin": {"guid": "0000000000000000f000000000000000", "file_id": "10720"},
            "source_revision": "unity-2022.3.21f1", "color_formula": "documentation only",
            "rgb_premultiply": False, "output_policy": "primary-color", "outputs": [0],
            "environment": {
                "fog": {"host_slot": "source-scene-fog", "required": True, "target": "black"},
                "soft_particles": {"host_slot": "source-soft-particles", "required": True,
                                   "formula": "documentation only", "depth_required_when_enabled": True},
            },
        }

    def test_both_legacy_models_have_distinct_source_identity_and_fog_contract(self):
        recipe = self.recipe()
        self.validator.validate(recipe)
        recipe["model"] = "unity-legacy-particle-alpha-blend"
        recipe["source_builtin"]["file_id"] = "10721"
        recipe["environment"]["fog"]["target"] = "scene-fog-color"
        self.validator.validate(recipe)
        recipe["source_builtin"]["file_id"] = "10720"
        self.assertFalse(self.validator.is_valid(recipe))

    def test_missing_environment_wrong_ids_unknown_models_and_code_are_rejected(self):
        for mutate in [
            lambda r: r.pop("environment"),
            lambda r: r["source_builtin"].update(file_id=10720),
            lambda r: r["source_builtin"].update(guid="0" * 32),
            lambda r: r["environment"]["soft_particles"].update(required=False),
            lambda r: r["environment"]["fog"].update(target="scene-fog-color"),
            lambda r: r.update(model="unknown"),
            lambda r: r.update(javascript="executable payload"),
        ]:
            recipe = copy.deepcopy(self.recipe())
            mutate(recipe)
            self.assertFalse(self.validator.is_valid(recipe), recipe)
