# multi-stage ビルド → distroless（非root）
FROM rust:1.88-alpine AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo install --path . --root /out --locked

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /out/bin/misskey-fetch /usr/local/bin/misskey-fetch
# TOKEN / API_URL / TZ はイメージに焼かず実行時 env 注入（TZ に既定はない。未設定は起動エラー）
USER nonroot
ENTRYPOINT ["/usr/local/bin/misskey-fetch"]
