"""把 index.json 里的下载地址**真下载一遍**，核对字节数。

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

清单里**没有 sha256**（2026-10-26 精简：一句 `size` 顶掉 `file` + `sha256` 两个字段，
理由见 `scripts/make_index.py` 头部）。脚本仍会打印实收字节的 sha256：人工核对、
或者哪天要把哈希加回清单时，这里就是现成的出处。

用法（从仓库根）：
    py scripts/check_urls.py            # 全部；未发布只提示，镜像未同步只 WARN
    py scripts/check_urls.py --strict   # 未发布 / 镜像未同步也判失败（发布前体检用）
    py scripts/check_urls.py common     # 只查指定 id
"""
import hashlib
import io
import json
import sys
import urllib.error
import urllib.request

INDEX = 'index.json'
CHUNK = 1 << 20
TIMEOUT = 60


def fetch(url):
    """流式下载，返回 (status, content_length, sha256, 实收字节数)。"""
    req = urllib.request.Request(url, headers={'User-Agent': 'RuPix-check-urls/1'})
    h = hashlib.sha256()
    got = 0
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        status = r.status
        length = r.headers.get('Content-Length')
        while True:
            buf = r.read(CHUNK)
            if not buf:
                break
            h.update(buf)
            got += len(buf)
    return status, (int(length) if length else None), h.hexdigest(), got


def validate(pid, url, size, label, bad):
    """下载一条地址并核对状态/字节数；不过把问题写进 `bad`，返回 None。"""
    print(f'  GET  {pid} [{label}]: {url}')
    try:
        status, length, digest, got = fetch(url)
    except urllib.error.HTTPError as e:
        bad.append(f'{pid}/{label}: HTTP {e.code}')
        print(f'       FAIL HTTP {e.code}')
        return
    except Exception as e:  # 网络/超时/证书
        bad.append(f'{pid}/{label}: {e}')
        print(f'       FAIL {e}')
        return

    problems = []
    if status != 200:
        problems.append(f'HTTP {status}')
    if length is not None and length != size:
        problems.append(f'Content-Length {length:,} != 清单 {size:,}')
    if got != size:
        problems.append(f'实收 {got:,} B != 清单 {size:,} B')
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
              f'{type(doc).__name__}。先跑 py scripts/make_index.py。')
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
        validate(pid, gitee, ent['size'], 'gitee', bad)
        # github 镜像：有则查；没有则 WARN（不算硬失败，除非 --strict）。
        if not github:
            unsynced.append(pid)
            print('       WARN github 镜像未同步（gitee 已发布，github 为空）'
                  '—— 切到 GitHub 来源时这枚会显示成灰的「下载」')
        else:
            validate(pid, github, ent['size'], 'github', bad)

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
