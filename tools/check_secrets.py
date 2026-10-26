"""本仓库的「无密钥 / 无隐私」门禁：`violations: 0` 才算过。

本仓是**公开**源码仓（gitee）。一旦有令牌、口令、私人邮箱、含真名的本机路径混进来，
删文件并不算解决——它已经进了 push 历史。所以这条门禁的价值在于**push 之前**拦住。

判据（只有这几条，宁可少报也不误报到没人看）：
  1. 私钥块            `-----BEGIN ... PRIVATE KEY-----`
  2. 令牌前缀          ghp_ / github_pat_ / glpat- / xox?- / AKIA… / sk-…
  3. URL 内嵌凭据      `scheme://user:pass@host`
  4. 口令字面量        键名像凭据、值又是**带引号的字面量**（`password: "hunter2x"`）；
                       只是把变量/字段赋来赋去（`password: self.config.password`）不算
  5. 邮箱              除三方署名文件外一律报（本仓没有合法的自有邮箱）
  6. 本机用户目录      `C:\\Users\\<名字>`（`Someone`/`<user>`/%USERNAME% 之类占位符豁免）

豁免范围：`.git/`、构建产物、`*.dll`、`testdata/`（二进制样本）、`Cargo.lock`（哈希）。
三方署名里的上游邮箱不豁免判据 5 —— 本仓 `THIRD-PARTY-NOTICES.md` 里若出现上游邮箱，
写进 `EMAIL_OK_FILES` 白名单，并在注释里说明为什么它必须留（法律要求归属）。

用法（从仓库根）：
    python tools/check_secrets.py          # 0 = 干净；1 = 有违规（逐条打印 文件:行）
    python tools/check_secrets.py --list   # 只列被扫的文件数，不判
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

SKIP_DIRS = {'.git', 'target', 'dist', '__pycache__', '.cargo', 'node_modules'}
SKIP_SUFFIX = {'.dll', '.pdb', '.exe', '.lib', '.obj', '.dylib', '.so', '.a',
               '.png', '.jpg', '.jpeg', '.gif', '.bmp', '.tif', '.tiff', '.webp',
               '.qoi', '.ico', '.psd', '.hdr', '.exr', '.dcm', '.ktx2', '.jxr', '.jp2'}
SKIP_NAMES = {'Cargo.lock'}
# 本文件自身必须豁免：它把每条判据的正则**原样写着**（`glpat-` 之类），不跳过就等于
# 永远自报违规。代价是"往这个文件里塞密钥"扫不出来——它是最显眼的一个文件，
# 评审看它比看门禁更靠谱。
SELF = pathlib.Path(__file__).resolve()
# 允许出现邮箱的文件（三方署名 / 许可证正文）。加进来的每一条都要有理由。
EMAIL_OK_FILES = {'THIRD-PARTY-NOTICES.md', 'LICENSE'}

TEXT_SUFFIX = {'.rs', '.toml', '.md', '.json', '.ps1', '.bat', '.py', '.c', '.h',
               '.txt', '.yml', '.yaml', '.jsonl', '.gitignore', '.gitattributes', ''}

PRIVATE_KEY = re.compile(r'-----BEGIN [A-Z ]*PRIVATE KEY-----')
# 前缀必须带够长的实体才算：文档里只写 `glpat-` 这个前缀不该被判成令牌（真实令牌 ≥20 位）。
TOKEN_PREFIX = re.compile(
    r'\b(?:ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|ghu_[A-Za-z0-9]{20,}'
    r'|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{16,}|xox[baprs]-[A-Za-z0-9-]{10,}'
    r'|AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{20,})')
URL_CREDS = re.compile(r'\b[a-z][a-z0-9+.-]*://[^\s/@:]+:[^\s/@]+@')
CRED_ASSIGN = re.compile(
    r'(?i)\b(?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key)\b'
    r'\s*[:=]\s*["\'][^"\']{8,}["\']')
EMAIL = re.compile(r'\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b')
USER_PATH = re.compile(r'[A-Za-z]:\\\\?Users\\\\?([A-Za-z0-9._-]+)')
USER_PATH_OK = {'Someone', 'someone', 'user', 'User', 'username', 'USERNAME', 'Public', 'Default'}


def iter_files():
    for p in sorted(ROOT.rglob('*')):
        if not p.is_file():
            continue
        if p.resolve() == SELF:
            continue
        if any(part in SKIP_DIRS for part in p.parts):
            continue
        if p.name in SKIP_NAMES:
            continue
        if p.suffix.lower() in SKIP_SUFFIX:
            continue
        if p.suffix.lower() not in TEXT_SUFFIX and p.name not in ('.gitignore', '.gitattributes'):
            continue
        yield p


def scan():
    bad = []
    scanned = 0
    for p in iter_files():
        scanned += 1
        rel = p.relative_to(ROOT).as_posix()
        try:
            text = p.read_text(encoding='utf-8', errors='replace')
        except OSError as e:
            bad.append((rel, 0, f'unreadable: {e}'))
            continue
        for i, line in enumerate(text.splitlines(), 1):
            if PRIVATE_KEY.search(line):
                bad.append((rel, i, 'private key block'))
            if TOKEN_PREFIX.search(line):
                bad.append((rel, i, 'access token prefix'))
            if URL_CREDS.search(line):
                bad.append((rel, i, 'credentials inside a url'))
            if CRED_ASSIGN.search(line):
                bad.append((rel, i, 'literal credential assignment'))
            if p.name not in EMAIL_OK_FILES:
                m = EMAIL.search(line)
                if m:
                    bad.append((rel, i, f'email: {m.group(0)}'))
            m = USER_PATH.search(line)
            if m and m.group(1) not in USER_PATH_OK:
                bad.append((rel, i, f'absolute user path: {m.group(0)}'))
    return scanned, bad


def main(argv):
    scanned, bad = scan()
    if '--list' in argv:
        print(f'files scanned: {scanned}')
        return 0
    if bad:
        print(f'secret violations: {len(bad)}')
        for rel, i, why in bad:
            print(f'  [x] {rel}:{i} -- {why}')
        return 1
    print(f'files scanned: {scanned}')
    print('violations: 0')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
