# 合并 .tasks/i18n/*.json 词条片段到 assets/locales/zh-CN.json（lead.json 优先，冲突会打印出来）。
import json
import pathlib
import re

here = pathlib.Path(__file__).resolve().parent
root = here.parent.parent
files = [here / "lead.json"] + sorted(p for p in here.glob("*.json") if p.name != "lead.json")
merged = {}
for path in files:
    data = json.loads(path.read_text(encoding="utf-8"))
    for key, value in data.items():
        if key in merged:
            if merged[key] != value:
                print(f"CONFLICT [{path.name}] {key!r}: {merged[key]!r} vs {value!r}")
            continue
        placeholders = set(re.findall(r"\{(\w+)\}", key))
        missing = [p for p in placeholders if "{" + p + "}" not in value]
        if missing:
            print(f"PLACEHOLDER [{path.name}] {key!r} -> {value!r} missing {missing}")
        merged[key] = value
out = root / "assets" / "locales" / "zh-CN.json"
out.write_text(json.dumps(merged, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print("zh-CN.json entries:", len(merged))
