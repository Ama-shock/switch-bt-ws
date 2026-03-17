# Dockerfile
#
# switch-bt-ws クロスコンパイルビルド環境
#
# ビルドツールチェインと BTStack ソースのみをイメージに含む。
# アプリケーションソースは docker compose up 時にボリュームマウントされる。
#
# ビルド方法:
#   docker compose up build
#
# 成果物:
#   dist/switch-bt-ws-v<version>.exe

FROM rust:1.77-slim-bookworm

# ---------------------------------------------------------------------------
# 依存パッケージのインストール
# ---------------------------------------------------------------------------
RUN apt-get update && apt-get install -y --no-install-recommends \
        gcc-mingw-w64-x86-64 \
        mingw-w64-x86-64-dev \
        git \
        jq \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Linux のケースセンシティブなファイルシステムへの対応。
RUN cd /usr/x86_64-w64-mingw32/include && \
    ln -sf windows.h  Windows.h  && \
    ln -sf winusb.h   Winusb.h   && \
    ln -sf setupapi.h SetupAPI.h

# Windows クロスコンパイル用ターゲットを追加
RUN rustup target add x86_64-pc-windows-gnu

# ---------------------------------------------------------------------------
# BTStack ソースの取得 (bluekitchen/btstack upstream)
# ---------------------------------------------------------------------------
WORKDIR /btstack
ARG BTSTACK_TAG=v1.5.3
RUN git clone --depth 1 --branch ${BTSTACK_TAG} https://github.com/bluekitchen/btstack.git windows

ENV BTSTACK_ROOT=/btstack/windows

# ---------------------------------------------------------------------------
# Cargo レジストリの事前フェッチ（キャッシュ効率のため）
# ---------------------------------------------------------------------------
WORKDIR /app

# エントリーポイント: マウントされたソース上でビルドを実行
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
