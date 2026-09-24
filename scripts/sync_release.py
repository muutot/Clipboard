"""Sync GitHub Releases (with assets) to GitCode Releases.

Usage:
  Local full sync (all missing tags):
    set GITCODE_TOKEN=xxx && python scripts/sync_release.py

  CI single-tag sync (sync-gitcode.yml, manual workflow_dispatch):
    SYNC_TAG=v1.2.3 python scripts/sync_release.py

All configuration comes from environment variables — no secrets in this file.
Required: GITCODE_TOKEN. Everything else has sane defaults/can be derived.
"""

import os
import sys
import tempfile
import time

import requests
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry
from tqdm import tqdm

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# ================= 配置区域 (全部走环境变量，无硬编码密钥) =================
# GITHUB_REPOSITORY 由 CI 传入 (形如 "owner/repo")，优先级高于下面两项。
_GITHUB_REPOSITORY = os.environ.get("GITHUB_REPOSITORY", "")
if "/" in _GITHUB_REPOSITORY:
    _DEFAULT_GH_OWNER, _DEFAULT_GH_REPO = _GITHUB_REPOSITORY.split("/", 1)
else:
    _DEFAULT_GH_OWNER, _DEFAULT_GH_REPO = "muutot", "clipboard"

GITHUB_OWNER = os.environ.get("GITHUB_OWNER", _DEFAULT_GH_OWNER)
GITHUB_REPO = os.environ.get("GITHUB_REPO", _DEFAULT_GH_REPO)
# 公开仓库可为空；私有仓库必填。CI 里传 secrets.GITHUB_TOKEN。
GITHUB_TOKEN = os.environ.get("GITHUB_TOKEN", "")

GITCODE_OWNER = os.environ.get("GITCODE_OWNER", "")
GITCODE_REPO = os.environ.get("GITCODE_REPO", "")
# GitCode 个人访问令牌 (必填)。CI 里传 secrets.GITCODE_TOKEN。
GITCODE_TOKEN = os.environ.get("GITCODE_TOKEN", "")

# CI 只同步当前发布的 tag；本地留空则同步全部缺失 tag。
SYNC_TAG = os.environ.get("SYNC_TAG", "").strip()

# 代理：CI runner 直连，默认不设；仅在需要时通过标准环境变量配置。
# e.g. set HTTPS_PROXY=http://127.0.0.1:7890
PROXIES = {
    key: os.environ[key]
    for key in ("HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy")
    if os.environ.get(key)
} or None

# Base API 路径 (一般不需要改)
GITHUB_API_BASE = os.environ.get("GITHUB_API_BASE", "https://api.github.com")
GITCODE_API_BASE = os.environ.get("GITCODE_API_BASE", "https://gitcode.com/api/v5")

# CI 日志里关掉进度条：NO_PROGRESS=1
NO_PROGRESS = os.environ.get("NO_PROGRESS", "1" if os.environ.get("CI") else "0") == "1"
# ============================================


def get_robust_session():
    """创建一个带有自动重试机制的 requests Session"""
    session = requests.Session()
    retries = Retry(
        total=5,
        backoff_factor=1,
        status_forcelist=[500, 502, 503, 504],
        raise_on_status=False,
    )
    adapter = HTTPAdapter(max_retries=retries)
    session.mount("http://", adapter)
    session.mount("https://", adapter)
    if PROXIES:
        session.proxies.update(PROXIES)
    session.trust_env = False  # 代理只走上面的显式配置，避免误读系统代理
    return session


http = get_robust_session()


def get_github_headers():
    headers = {"Accept": "application/vnd.github.v3+json"}
    if GITHUB_TOKEN and GITHUB_TOKEN.strip():
        headers["Authorization"] = f"Bearer {GITHUB_TOKEN.strip()}"
    return headers


def get_gitcode_headers():
    return {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {GITCODE_TOKEN.strip()}",
    }


def download_file_with_progress_and_resume(
    download_url, local_path, asset_name, max_attempts=10
):
    """
    带有 tqdm 进度条 + HTTP Range 断点续传的下载函数
    """
    download_headers = {}
    if GITHUB_TOKEN and GITHUB_TOKEN.strip():
        download_headers["Authorization"] = f"Bearer {GITHUB_TOKEN.strip()}"

    for attempt in range(1, max_attempts + 1):
        file_mode = "ab"  # 追加写入模式
        downloaded_bytes = 0

        if os.path.exists(local_path):
            downloaded_bytes = os.path.getsize(local_path)

        headers = download_headers.copy()
        if downloaded_bytes > 0:
            headers["Range"] = f"bytes={downloaded_bytes}-"

        try:
            with http.get(
                download_url, headers=headers, stream=True, timeout=(15, 60)
            ) as r:
                if r.status_code == 416:
                    print(f"    [OK] {asset_name} 已完整下载，无需重复拉取。")
                    return True

                # 服务器忽略 Range 请求返回 200 时，本地半截文件不能继续追加，
                # 必须丢弃旧分片从头重写，否则会拼出损坏的资产文件。
                if r.status_code == 200 and downloaded_bytes > 0:
                    file_mode = "wb"
                    downloaded_bytes = 0

                if r.status_code not in (200, 206):
                    print(f"    [!] 服务器返回状态码: {r.status_code}")

                r.raise_for_status()

                # 获取文件总大小 (如果是 206 断点续传，Content-Length 是剩余部分大小)
                content_length = int(r.headers.get("Content-Length", 0))
                total_size = (
                    content_length + downloaded_bytes if content_length else None
                )

                chunk_size = 1024 * 1024  # 1MB

                # 初始化 tqdm 进度条
                with tqdm(
                    total=total_size,
                    initial=downloaded_bytes,
                    unit="B",
                    unit_scale=True,
                    unit_divisor=1024,
                    desc=f"    [dl] {asset_name[:20]}...",
                    leave=True,
                    ncols=80,
                    disable=NO_PROGRESS,
                ) as pbar:
                    with open(local_path, file_mode) as f:
                        for chunk in r.iter_content(chunk_size=chunk_size):
                            if chunk:
                                f.write(chunk)
                                f.flush()
                                pbar.update(len(chunk))  # 更新进度条

            print(f"    [OK] 下载成功: {asset_name}")
            return True

        except (requests.exceptions.RequestException, Exception) as e:
            current_size = (
                os.path.getsize(local_path) if os.path.exists(local_path) else 0
            )
            print(
                f"\n    [!] 第 {attempt}/{max_attempts} 次下载中断 ({type(e).__name__})，"
                f"已保存 {current_size / (1024 * 1024):.2f} MB，3秒后续传..."
            )
            time.sleep(3)

    return False


def fetch_github_release_by_tag(tag_name):
    """按 tag 获取单条 GitHub Release (草稿也可，token 需有读权限)。"""
    url = (
        f"{GITHUB_API_BASE}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/tags/{tag_name}"
    )
    print(
        f"[*] 正在从 GitHub 获取 Release: {GITHUB_OWNER}/{GITHUB_REPO}@{tag_name} ..."
    )
    response = http.get(url, headers=get_github_headers(), timeout=(10, 30))
    if response.status_code != 200:
        print(f"[-] 获取 GitHub Release 失败 [{response.status_code}]: {response.text}")
        return None
    return response.json()


def fetch_all_github_releases():
    """获取 GitHub 仓库的所有 Release"""
    url = f"{GITHUB_API_BASE}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases"
    releases = []
    page = 1

    print(f"[*] 正在从 GitHub 获取 Release: {GITHUB_OWNER}/{GITHUB_REPO} ...")

    while True:
        response = http.get(
            url,
            headers=get_github_headers(),
            params={"page": page, "per_page": 100},
            timeout=(10, 30),
        )
        if response.status_code != 200:
            print(
                f"[-] 获取 GitHub Release 失败 [{response.status_code}]: {response.text}"
            )
            break

        data = response.json()
        if not data:
            break

        releases.extend(data)
        page += 1

    print(f"[+] 成功获取 {len(releases)} 个 GitHub Release。")
    return releases


def fetch_existing_gitcode_tags():
    """获取 GitCode 上已存在的 Release Tag 列表"""
    url = f"{GITCODE_API_BASE}/repos/{GITCODE_OWNER}/{GITCODE_REPO}/releases"
    response = http.get(url, headers=get_gitcode_headers(), timeout=(10, 30))

    if response.status_code == 200:
        releases = response.json()
        return {rel.get("tag_name") for rel in releases if isinstance(rel, dict)}
    else:
        print(
            f"[!] 读取 GitCode 已有 Release 失败 (HTTP {response.status_code})，将尝试直接同步。"
        )
        return set()


def upload_asset_to_gitcode_by_tag(tag_name, file_path, file_name):
    """通过 GitCode 的 upload_url 两步法上传附件（GET 拿地址 + PUT 上传）"""
    headers = {"Authorization": f"Bearer {GITCODE_TOKEN.strip()}"}
    file_size = os.path.getsize(file_path)

    # 1. 获取上传地址
    upload_url_endpoint = (
        f"{GITCODE_API_BASE}/repos/{GITCODE_OWNER}/{GITCODE_REPO}"
        f"/releases/{tag_name}/upload_url"
    )
    print(f"    [up] 正在获取上传地址: {file_name} ...")
    resp = http.get(
        upload_url_endpoint,
        headers=headers,
        params={"file_name": file_name},
        timeout=(10, 30),
    )
    if resp.status_code != 200:
        print(
            f"    [x] 获取上传地址失败 ({file_name}) [{resp.status_code}]: {resp.text}"
        )
        return False

    upload_info = resp.json()
    if isinstance(upload_info, str):
        upload_url = upload_info
        upload_headers = {}
    else:
        upload_url = upload_info.get("upload_url") or upload_info.get("url")
        upload_headers = upload_info.get("headers") or {}

    if not upload_url:
        print(f"    [x] 响应中未找到 upload_url: {resp.text}")
        return False

    # 2. PUT 上传文件内容（带上 OBS 要求的 headers）
    print(
        f"    [up] 正在上传至 GitCode: {file_name} ({file_size / (1024 * 1024):.2f} MB) ..."
    )
    put_headers = {"Authorization": f"Bearer {GITCODE_TOKEN.strip()}"}
    put_headers.update(upload_headers)
    with open(file_path, "rb") as f:
        resp = http.put(upload_url, headers=put_headers, data=f, timeout=(15, 600))

    if resp.status_code in (200, 201):
        print(f"    [OK] 成功上传附件: {file_name}")
        return True
    else:
        print(f"    [x] 上传附件失败 ({file_name}) [{resp.status_code}]: {resp.text}")
        return False


def ensure_gitcode_release(tag_name, release_name, release_body, prerelease, draft):
    """在 GitCode 创建 Release；已存在则视为成功，继续传附件。"""
    create_url = f"{GITCODE_API_BASE}/repos/{GITCODE_OWNER}/{GITCODE_REPO}/releases"
    payload = {
        "tag_name": tag_name,
        "name": release_name,
        "body": release_body,
        "prerelease": prerelease,
        "draft": draft,
    }

    resp = http.post(
        create_url, headers=get_gitcode_headers(), json=payload, timeout=(10, 30)
    )

    if resp.status_code in (200, 201):
        print(f"[+] Release [{tag_name}] 主体创建成功！")
        return True
    if resp.status_code == 422 or "already" in resp.text.lower():
        print(f"[=] Release [{tag_name}] 在 GitCode 已存在，继续同步附件。")
        return True
    print(
        f"[-] 创建 GitCode Release [{tag_name}] 失败 [{resp.status_code}]: {resp.text}"
    )
    return False


def sync_release(gh_release):
    """同步单条 Release，返回 True/False。"""
    tag_name = gh_release.get("tag_name")
    release_name = gh_release.get("name") or tag_name
    release_body = gh_release.get("body") or ""
    prerelease = gh_release.get("prerelease", False)
    # GitHub 侧是 draft 时，GitCode 侧同样建为 draft，避免未发布先公开。
    draft = gh_release.get("draft", False)

    print(f"\n[->] 开始同步 Release: {tag_name}")

    # 1. 在 GitCode 创建 Release（已存在则复用）
    if not ensure_gitcode_release(
        tag_name, release_name, release_body, prerelease, draft
    ):
        return False

    # 2. 处理 Release 随附文件 (Assets)
    assets = gh_release.get("assets", [])
    if not assets:
        print("    [i] 无附件需要同步。")
        return True

    print(f"    [i] 发现 {len(assets)} 个附件，准备同步...")

    ok = True
    with tempfile.TemporaryDirectory() as temp_dir:
        for asset in assets:
            asset_name = asset.get("name")
            download_url = asset.get("browser_download_url")
            local_path = os.path.join(temp_dir, asset_name)

            success = download_file_with_progress_and_resume(
                download_url, local_path, asset_name
            )

            if success:
                if not upload_asset_to_gitcode_by_tag(tag_name, local_path, asset_name):
                    ok = False
            else:
                print(f"    [x] 附件 ({asset_name}) 下载未完成，跳过上传。")
                ok = False
    return ok


def main():
    if not GITHUB_OWNER or not GITHUB_REPO:
        print("[!] 错误: 请设置 GITHUB_OWNER 与 GITHUB_REPO (或 GITHUB_REPOSITORY)！")
        return 1

    if not GITCODE_TOKEN or not GITCODE_OWNER or not GITCODE_REPO:
        print(
            "[!] 错误: 请设置 GitCode 配置 "
            "(GITCODE_OWNER, GITCODE_REPO, GITCODE_TOKEN)！"
        )
        return 1

    # CI 单 tag 模式：只同步当前发布的版本，失败直接非零退出。
    if SYNC_TAG:
        gh_release = fetch_github_release_by_tag(SYNC_TAG)
        if not gh_release:
            return 1
        return 0 if sync_release(gh_release) else 1

    # 本地全量模式：同步所有缺失的 tag。
    gh_releases = fetch_all_github_releases()
    if not gh_releases:
        print("[-] 未在 GitHub 找到任何 Release。")
        return 1

    existing_tags = fetch_existing_gitcode_tags()

    failed = []
    for gh_release in reversed(gh_releases):
        tag_name = gh_release.get("tag_name")
        if tag_name in existing_tags:
            print(f"[=] 跳过已存在的 Release: {tag_name}")
            continue

        if not sync_release(gh_release):
            failed.append(tag_name)

    if failed:
        print(f"\n[x] 以下 Release 同步失败: {failed}")
        return 1

    print("\n[OK] 所有 Release 同步任务完成！")
    return 0


if __name__ == "__main__":
    sys.exit(main())
