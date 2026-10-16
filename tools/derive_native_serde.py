"""Add Serialize to flatc native object types for inspect reports (not wire JSON)."""
import re
from pathlib import Path
for path in Path('crates/noa-format/src/generated').glob('*_generated.rs'):
    text = path.read_text()
    text = re.sub(r'#\[derive\(([^\n]*)\)\]\n(pub (?:struct|enum) \w+T\b)',
                  lambda m: '#[derive(' + m[1] + ', serde::Serialize)]\n' + m[2], text)
    text = text.replace("self.variant_name().unwrap()", 'self.variant_name().ok_or_else(|| serde::ser::Error::custom("unknown enum value"))?')
    path.write_text(text)
