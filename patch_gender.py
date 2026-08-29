import re, subprocess, pathlib, collections

def build():
    r = subprocess.run(["cargo","build","--workspace","--all-targets","--message-format=short"],
                       capture_output=True, text=True, encoding="utf-8", errors="replace")
    return r.stdout + r.stderr

for rnd in range(15):
    out = build()
    locs = set()
    for m in re.finditer(r"^(.*?\.rs):(\d+):(\d+): error\[E0063\]: missing field `gender`", out, re.M):
        locs.add((m.group(1), int(m.group(2))))
    if not locs:
        print("DONE round", rnd)
        for l in out.splitlines():
            if "error" in l:
                print(l)
        break
    byfile = collections.defaultdict(list)
    for f, ln in locs:
        byfile[f].append(ln)
    for f, lns in byfile.items():
        p = pathlib.Path(f)
        lines = p.read_text(encoding="utf-8").split("\n")
        for ln in sorted(lns, reverse=True):
            idx = ln - 1
            line = lines[idx]
            assert "Agent {" in line or "NpcProfile {" in line, (f, ln, line)
            indent = re.match(r"\s*", line).group(0) + "    "
            path = "crate::entity::Gender" if f.replace("\\","/").startswith("crates/ll-world/") else "ll_world::entity::Gender"
            lines.insert(idx+1, f"{indent}// 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。")
            lines.insert(idx+2, f"{indent}gender: {path}::default(),")
        p.write_text("\n".join(lines), encoding="utf-8")
        print("patched", f, sorted(lns))
