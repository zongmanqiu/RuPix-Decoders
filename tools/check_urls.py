"""把 decoders.json 里的下载地址**真下载一遍**，核对字节数。

为什么要这个脚本：清单的 `gitee` / `github` 是人工填的（生成器没有网络，不臆造地址）。
填错的后果不是"下不了"这么简单——宿主的「解码器 / 可下载」卡会列出一排点了 404 的按钮，
而作者本地一切正常。2026-10-26 就撞过：旧清单里 pro/raw 指向一个不存在的批次 tag，
四条地址里两条是 404，直到手工 curl 才发现。

每个条目有**两列地址**（`gitee` 主源、`github` 镜像），对应宿主里「来源」下拉的两个选项。
判据（任何一条不过即退出码 1）：
  1. HTTP 200（跟随重定向；gitee / github release 资产都会跳一次）；
  2. `Content-Length` 与清单 `size` 一致；
  3. 实际下载字节数与该 `size` 一致。

`gitee` 是主发布源：没有 `gitee` 的条目算"未发布"，只提示、在 `--strict` 下才判失败。
`github` 是镜像：有 `gitee` 但 `github` 为空 = "镜像还没同步"，只 WARN（切到 GitHub 来源时
那枚会显示成灰的「下载」），`--strict` 下判失败。

清单里有 `sha256`：脚本把实收字节的哈希与清单逐项核对；`size` 挡截断/多收，
`sha256` 挡同字节数替换。`file` 仍然不写（文件名可由 id + version 算出）。

用法（从仓库根）：
    py tools/check_urls.py            # 全部；未发布只提示，镜像未同步只 WARN
    py tools/check_urls.py --strict   # 未发布 / 镜像未同步也判失败（发布前体检用）
    py tools/check_urls.py common     # 只查指定 id
"""
import hashlib
import io
import json
import sys
import zipfile
import urllib.error
import urllib.request

INDEX = 'decoders.json'
CHUNK = 1 << 20
TIMEOUT = 60


def fetch(url):
    """Stream a URL and return (status, content_length, body_bytes)."""
    req = urllib.request.Request(url, headers={'User-Agent': 'RuPix-check-urls/1'})
    body = bytearray()
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        status = r.status
        length = r.headers.get('Content-Length')
        while True:
            buf = r.read(CHUNK)
            if not buf:
                break
            body.extend(buf)
    return status, (int(length) if length else None), bytes(body)


def validate(pid, url, version, size, expected_sha, label, bad):
    """Download one asset; ZIP assets are checked by their DLL payload."""
    print(f'  GET  {pid} [{label}]: {url}')
    try:
        status, length, body = fetch(url)
    except urllib.error.HTTPError as e:
        bad.append(f'{pid}/{label}: HTTP {e.code}')
        print(f'       FAIL HTTP {e.code}')
        return
    except Exception as e:
        bad.append(f'{pid}/{label}: {e}')
        print(f'       FAIL {e}')
        return

    problems = []
    if status != 200:
        problems.append(f'HTTP {status}')
    is_zip = url.split('?', 1)[0].lower().endswith('.zip')
    if is_zip:
        dll_name = f'{pid}_{version}.dll'
        payload = b''
        try:
            with zipfile.ZipFile(io.BytesIO(body)) as z:
                names = z.namelist()
                if dll_name not in names:
                    problems.append(f'ZIP missing {dll_name}')
                for name in names:
                    if name.startswith('/') or '..' in name.split('/'):
                        problems.append(f'unsafe ZIP path {name}')
                    if name.lower().endswith('.dll') and name != dll_name:
                        problems.append(f'extra DLL in ZIP {name}')
                if dll_name in names:
                    payload = z.read(dll_name)
        except zipfile.BadZipFile as e:
            problems.append(f'bad ZIP: {e}')
        got = len(payload)
        digest = hashlib.sha256(payload).hexdigest()
        print(f'       ZIP {len(body):,} B; DLL {got:,} B')
        if length is not None:
            print(f'       ZIP Content-Length {length:,}')
    else:
        got = len(body)
        digest = hashlib.sha256(body).hexdigest()
        if length is not None and length != size:
            problems.append(f'Content-Length {length:,} != list {size:,}')
    if got != size:
        problems.append(f'DLL size {got:,} B != list {size:,} B')
    if expected_sha and digest.lower() != expected_sha.lower():
        problems.append(f'sha256 {digest[:16]}… != list {expected_sha[:16]}…')
    if problems:
        bad.append(f'{pid}/{label}: ' + '; '.join(problems))
        print('       FAIL ' + '; '.join(problems))
    else:
        print(f'       OK   {got:,} B  sha256 {digest[:16]}…')


def main(argv):
    strict = '--strict' in argv
    only = [a for a in argv[1:] if not a.startswith('-')]

    with io.open(INDEX, encoding='utf-8-sig') as f:
        doc = json.load(f)
    if not isinstance(doc, list):
        print('清单顶层应当是数组（一项一枚 DLL）；见到的是 '
              f'{type(doc).__name__}。先跑 py tools/make_index.py。')
        return 1

    bad, unpublished, unsynced = [], [], []
    for ent in doc:
        pid = ent['id']
        if only and pid not in only:
            continue
        gitee = ent.get('gitee') or ''
        github = ent.get('github') or ''
        if not gitee:
            unpublished.append(pid)
            extra = f'；但填了 github={github}' if github else ''
            print(f'  SKIP {pid}: 未发布（gitee 无地址）{extra}')
            continue
        # gitee 主源：必查。
        validate(pid, gitee, ent['version'], ent['size'], ent.get('sha256', ''), 'gitee', bad)
        # github 镜像：有则查；没有则 WARN（不算硬失败，除非 --strict）。
        if not github:
            unsynced.append(pid)
            print('       WARN github 镜像未同步（gitee 已发布，github 为空）'
                  '—— 切到 GitHub 来源时这枚会显示成灰的「下载」')
        else:
            validate(pid, github, ent['version'], ent['size'], ent.get('sha256', ''), 'github', bad)

    print()
    if unpublished:
        print(f'未发布（{len(unpublished)}）：' + ', '.join(unpublished))
    if unsynced:
        print(f'github 镜像未同步（{len(unsynced)}）：' + ', '.join(unsynced))
    if bad:
        print(f'FAILED {len(bad)}:')
        for b in bad:
            print('  - ' + b)
        return 1
    if strict and (unpublished or unsynced):
        print('--strict：存在未发布 / 镜像未同步条目，判失败')
        return 1
    print('urls: 0 failure(s)')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
