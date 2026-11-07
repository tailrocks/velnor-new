FROM ubuntu@sha256:42527649dbc4f9197a6073f426f488cbff0e9cc9f2afce32d91c383e4875634a AS runtime-base
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl dbus gcc libc6-dev libapparmor1 libaudit1 libblkid1 libbpf1 libc6 libcap-ng0 libcrypt1 libelf1t64 libkmod2 libmount1 libpcre2-8-0 libseccomp2 libselinux1 libssl3t64 libsystemd-shared libzstd1 openssl python3 sudo systemd systemd-sysv xz-utils zlib1g && rm -rf /var/lib/apt/lists/*
FROM runtime-base AS verified-payload
COPY --chown=0:0 rootfs/ /candidate/rootfs/
COPY --chown=0:0 proof.json global-policy.json verify-files.py /opt/velnor-manager-fixture/
RUN /usr/bin/python3 -I -S -B /opt/velnor-manager-fixture/verify-files.py /candidate/rootfs
FROM runtime-base
COPY --from=verified-payload --chown=0:0 /candidate/rootfs/ /
COPY --from=verified-payload --chown=0:0 /opt/velnor-manager-fixture/ /opt/velnor-manager-fixture/
ENV LANG=C.UTF-8 container=docker
LABEL io.velnor.fixture.role=manager-policy-check-only
STOPSIGNAL SIGRTMIN+3
CMD ["/usr/lib/systemd/systemd"]
