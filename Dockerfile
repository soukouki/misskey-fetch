# multi-stage ビルド → distroless（非root）
# uuid 1.28.0 の MSRV が rustc 1.89.0（Cargo.lock の他の依存は 1.88 で可）
FROM rust:1.99-alpine AS build
# proc-macro crate は .so として動的リンクされるため、crt1.o/crti.o を持つ musl-dev が必要
# （公式 rust:alpine イメージには入っていない。build stage 専用、+11MB 程度）
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo install --path . --root /out --locked

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /out/bin/misskey-fetch /usr/local/bin/misskey-fetch
# TOKEN / API_URL / TZ はイメージに焼かず実行時 env 注入（TZ に既定はない。未設定は起動エラー）
USER nonroot
ENTRYPOINT ["/usr/local/bin/misskey-fetch"]
