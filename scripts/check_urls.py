"""把 index.json 里的下载地址**真下载一遍**，核对字节数。

为什么要这个脚本：清单的 `url` 是人工填的（生成器没有网络，不臆造地址）。
填错的后果不是"下不了"这么简单——宿主的「解码器 / 可下载」卡会列出一排点了 404 的按钮，
而作者本地一切正常。2026-10-26 就撞过：旧清单里 pro/raw 指向一个不存在的批次 tag，
四条地址里两条是 404，直到手工 curl 才发现。

判据（任何一条不过即退出码 1）：
  1. HTTP 200（跟随重定向；gitee release 资产会跳一次）；
  2. `Content-Length` 与清单 `size` 一致；
  3. 实际下载字节数与该 `size` 一致。

清单里**没有 sha256**（2026-10-26 精简：一句 `size` 顶掉 `file` + `sha256` 两个字段，
理由见 `scripts/make_index.py` 头部）。脚本仍会打印实收字节的 sha256：人工核对、
或者哪天要把哈希加回清单时，这里就是现成的出处。

没有 `url` 的条目（尚未发布）只打印提示，不算失败——**清单如实说"还没发布"是对的**，
不能因此判红，否则大家会把地址乱填一气来消红。

用法（从仓库根）：
    py scripts/check_urls.py            # 全部；缺地址只提示
    py scripts/check_urls.py --strict   # 缺地址也判失败（发布前体检用）
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


def main(argv):
    strict = '--strict' in argv
    only = [a for a in argv[1:] if not a.startswith('-')]

    with io.open(INDEX, encoding='utf-8-sig') as f:
        doc = json.load(f)
    if not isinstance(doc, list):
        print('清单顶层应当是数组（一项一枚 DLL）；见到的是 '
              f'{type(doc).__name__}。先跑 py scripts/make_index.py。')
        return 1

    bad, unpublished = [], []
    for ent in doc:
        pid = ent['id']
        if only and pid not in only:
            continue
        url = ent.get('url') or ''
        if not url:
            unpublished.append(pid)
            print(f'  SKIP {pid}: 尚无下载地址（未发布）')
            continue
        print(f'  GET  {pid}: {url}')
        try:
            status, length, digest, got = fetch(url)
        except urllib.error.HTTPError as e:
            bad.append(f'{pid}: HTTP {e.code}')
            print(f'       FAIL HTTP {e.code}')
            continue
        except Exception as e:  # 网络/超时/证书
            bad.append(f'{pid}: {e}')
            print(f'       FAIL {e}')
            continue

        problems = []
        if status != 200:
            problems.append(f'HTTP {status}')
        if length is not None and length != ent['size']:
            problems.append(f'Content-Length {length:,} != 清单 {ent["size"]:,}')
        if got != ent['size']:
            problems.append(f'实收 {got:,} B != 清单 {ent["size"]:,} B')
        if problems:
            bad.append(f'{pid}: ' + '; '.join(problems))
            print('       FAIL ' + '; '.join(problems))
        else:
            print(f'       OK   {got:,} B  sha256 {digest[:16]}…')

    print()
    if unpublished:
        print(f'未发布（{len(unpublished)}）：' + ', '.join(unpublished))
    if bad:
        print(f'FAILED {len(bad)}:')
        for b in bad:
            print('  - ' + b)
        return 1
    if strict and unpublished:
        print('--strict：存在未发布条目，判失败')
        return 1
    print('urls: 0 failure(s)')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
