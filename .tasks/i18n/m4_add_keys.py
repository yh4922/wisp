# M4：向 zh-CN.json 追加工作区相关译文（已存在的 key 跳过，保持合法 JSON、无重复 key）。
import json, sys
from collections import OrderedDict

PATH = r"D:\Project\Wisp\assets\locales\zh-CN.json"
NEW = OrderedDict([
    ("SSH remote is coming soon", "SSH 远程即将支持"),
    ("Workspace name", "工作区名称"),
    ("Cancel", "取消"),
    ("Save", "保存"),
    ("Failed to open project", "打开项目失败"),
    ("Save Workspace", "保存工作区"),
    ("Save as Workspace", "保存为工作区"),
    ("Rename Workspace", "重命名工作区"),
    ('Delete workspace "{name}"?', "删除工作区“{name}”？"),
    ("Only the saved record is removed. Files on disk are not deleted.", "只删除保存的记录，不会删除磁盘上的文件。"),
    ("Delete", "删除"),
    ("Open Local Folder", "打开本地文件夹"),
    ("Open Remote", "打开远程"),
    ("Workspaces", "工作区"),
    ("Saved Workspaces", "已保存的工作区"),
    ("No saved workspaces yet. Open a local folder to add one.", "还没有保存的工作区，打开一个本地文件夹即可添加。"),
    ("Recently Opened", "最近打开"),
    ("Open in New Window", "在新窗口打开"),
    ("Rename", "重命名"),
    ("Add Workspace", "添加工作区"),
    ("Open Local Folder…", "打开本地文件夹…"),
    ("Open Remote…", "打开远程…"),
    ("Saved", "已保存"),
    ("No recent projects", "没有最近打开的项目"),
    ("Workspaces Panel", "工作区面板"),
])

def check_dupes(pairs):
    seen = OrderedDict()
    for k, v in pairs:
        if k in seen:
            raise SystemExit(f"duplicate key: {k!r}")
        seen[k] = v
    return seen

text = open(PATH, encoding="utf-8").read()
data = json.loads(text, object_pairs_hook=check_dupes)
added = []
for k, v in NEW.items():
    if k in data:
        print(f"exists: {k!r} = {data[k]!r}")
    else:
        data[k] = v
        added.append(k)
if "--write" in sys.argv:
    out = json.dumps(data, ensure_ascii=False, indent=2) + "\n"
    open(PATH, "w", encoding="utf-8", newline="\n").write(out)
    json.loads(open(PATH, encoding="utf-8").read(), object_pairs_hook=check_dupes)
print("added:", len(added))
