FROM mcr.microsoft.com/dotnet/sdk:8.0@sha256:1b955f179b02df4f0eb5fadedaa898cbe44d620a64aab52eec72c8e91d1355b5 AS build
ARG RUNNER_SERVER_COMMIT=992ccbbbf9afcde477c38c316e053b1af457ad40
WORKDIR /src
RUN git init -q . \
    && git fetch -q --depth 1 https://github.com/ChristopherHX/runner.server "${RUNNER_SERVER_COMMIT}" \
    && git checkout -q FETCH_HEAD \
    && test "$(git rev-parse HEAD)" = "${RUNNER_SERVER_COMMIT}"
RUN sed -i 's|container.StoreName)), "application/octet-stream")|container.StoreName)), "application/zip")|' src/Runner.Server/Controllers/ArtifactControllerV2.cs \
    && grep -q 'container.StoreName)), "application/zip")' src/Runner.Server/Controllers/ArtifactControllerV2.cs
RUN cd src && dotnet msbuild dir.proj -t:GenerateConstant \
    && dotnet publish Runner.Client -c Release --no-self-contained -p:BUILD_OS=Any -p:RuntimeFrameworkVersion=8.0.0 -o /out

FROM mcr.microsoft.com/dotnet/aspnet:8.0@sha256:dccacae9e0e103b5d3ba5fbe442dd8d3316b990cd8f2ec9be0a0ec9db0c0e520
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
COPY --from=build /out /opt/runner.server
WORKDIR /data
ENV ASPNETCORE_URLS=http://+:80
ENTRYPOINT ["dotnet", "/opt/runner.server/Runner.Server.dll"]
