FROM rust:1.94-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p pptalk-node

FROM debian:bookworm-slim
RUN useradd --system --uid 9464 --home /var/lib/pptalk-node pptalk
COPY --from=builder /src/target/release/pptalk-node /usr/local/bin/pptalk-node
USER pptalk
EXPOSE 9464
VOLUME ["/var/lib/pptalk-node"]
ENTRYPOINT ["/usr/local/bin/pptalk-node"]
CMD ["--listen", "0.0.0.0:9464", "--data-dir", "/var/lib/pptalk-node"]
