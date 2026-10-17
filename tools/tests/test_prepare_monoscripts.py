import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "prepare_monoscripts", Path(__file__).parents[1] / "prepare_monoscripts.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PrepareMonoScriptsTests(unittest.TestCase):
    def test_sample_sidecar_matches_original_monoscript_records(self):
        root = Path(__file__).parents[2]
        if not (root / "data/serialized-objects.json").exists():
            self.skipTest("local original MonoScript fixture is unavailable")
        expected = json.loads((root / "data/scenario/effect/kirakira_01_rip/unity.monoscripts.json").read_text())
        objects = json.loads((root / "data/serialized-objects.json").read_text())
        bindings = [[s["guid"], s["file_id"], s["source"]["asset"], s["source"]["file_id"]] for s in expected["scripts"]]
        self.assertEqual(module.prepare(objects, bindings), expected)

    def test_identity_comes_from_monoscript_and_ids_remain_strings(self):
        objects = {"objects": [{
            "sourceFile": "cab", "pathID": 9223372036854775807,
            "classID": 115, "type": "MonoScript",
            "data": {"m_AssemblyName": "Actual", "m_Namespace": "", "m_ClassName": "Mover"},
        }]}
        binding = ["a" * 32, "11500000", "cab", "9223372036854775807"]
        result = module.prepare(objects, [binding])
        self.assertEqual(result["scripts"][0]["assembly"], "Actual")
        self.assertEqual(result["scripts"][0]["source"]["file_id"], "9223372036854775807")
        self.assertEqual(result["scripts"][0]["file_id"], "11500000")
        objects["objects"][0]["classID"] = 114
        with self.assertRaises(ValueError):
            module.prepare(objects, [binding])

    def test_conflicting_source_mapping_is_rejected(self):
        objects = {"objects": [{
            "sourceFile": "cab", "pathID": i, "classID": 115, "type": "MonoScript",
            "data": {"m_AssemblyName": assembly, "m_Namespace": "", "m_ClassName": "Mover"},
        } for i, assembly in [(1, "First"), (2, "Second")]]}
        with self.assertRaises(ValueError):
            module.prepare(objects, [["a" * 32, "11500000", "cab", str(i)] for i in [1, 2]])

    def test_missing_sources_and_noncanonical_ids_are_rejected(self):
        objects = {"objects": [{
            "sourceFile": "cab", "pathID": 1, "classID": 115, "type": "MonoScript",
            "data": {"m_AssemblyName": "Actual", "m_Namespace": "", "m_ClassName": "Mover"},
        }]}
        for binding in [["a" * 32, "011500000", "cab", "1"],
                        ["a" * 32, "11500000", "cab", "01"],
                        ["a" * 32, 11500000, "cab", "1"],
                        ["0" * 32, "11500000", "cab", "1"],
                        ["a" * 32, "11500000", "", "1"]]:
            with self.assertRaises(ValueError):
                module.prepare(objects, [binding])
        with self.assertRaises(KeyError):
            module.prepare(objects, [["a" * 32, "11500000", "missing", "1"]])


if __name__ == "__main__":
    unittest.main()
