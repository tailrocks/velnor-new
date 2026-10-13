FROM scratch

ARG SOURCE_SHA
COPY resource-probe /velnor/resource-probe
WORKDIR /
USER 65532:65532
ENTRYPOINT ["/velnor/resource-probe"]
LABEL org.opencontainers.image.revision=${SOURCE_SHA}
