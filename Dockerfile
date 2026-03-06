# Dockerfile
#
# switch-bt-ws クロスコンパイルビルド環境
#
# Linux コンテナ内で以下を実行します:
#   1. mizuyoukanao/btstack を clone
#   2. WinUSB トランスポートにデバイス指定パッチを適用
#   3. x86_64-pc-windows-gnu ターゲットで cargo build --release
#
# ビルド方法:
#   docker build -t switch-bt-ws-builder .
#   docker run --rm -v "$(pwd)/dist:/out" switch-bt-ws-builder
#
# 成果物:
#   dist/switch-bt-ws.exe

FROM rust:1.77-slim-bookworm AS builder

# ---------------------------------------------------------------------------
# 依存パッケージのインストール
# ---------------------------------------------------------------------------
RUN apt-get update && apt-get install -y --no-install-recommends \
        gcc-mingw-w64-x86-64 \
        mingw-w64-x86-64-dev \
        git \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Linux のケースセンシティブなファイルシステムへの対応。
# BTStack は Windows 慣例の大文字ヘッダー名でインクルードするが、
# mingw-w64 のヘッダーはすべて小文字で格納されているためシンボリックリンクを作成する。
RUN cd /usr/x86_64-w64-mingw32/include && \
    ln -sf windows.h  Windows.h  && \
    ln -sf winusb.h   Winusb.h   && \
    ln -sf setupapi.h SetupAPI.h

# Windows クロスコンパイル用ターゲットを追加
RUN rustup target add x86_64-pc-windows-gnu

# ---------------------------------------------------------------------------
# BTStack ソースの取得
# ---------------------------------------------------------------------------
WORKDIR /btstack
# コミットを固定してビルドの再現性を確保する
ARG BTSTACK_COMMIT=a843d07e2
RUN git clone https://github.com/mizuyoukanao/btstack.git windows && \
    git -C windows checkout ${BTSTACK_COMMIT}

# ---------------------------------------------------------------------------
# アプリケーションソースのコピー
# ---------------------------------------------------------------------------
WORKDIR /app
COPY . .

# ---------------------------------------------------------------------------
# BTStack パッチの適用
# ---------------------------------------------------------------------------
RUN bash patches/apply_patches.sh /btstack/windows

# ---------------------------------------------------------------------------
# ビルド
# ---------------------------------------------------------------------------
ENV BTSTACK_ROOT=/btstack/windows
RUN cargo build --release --target x86_64-pc-windows-gnu

# ---------------------------------------------------------------------------
# 成果物をエクスポートステージへコピー
# ---------------------------------------------------------------------------
FROM scratch AS export
COPY --from=builder /app/target/x86_64-pc-windows-gnu/release/switch-bt-ws.exe /switch-bt-ws.exe
