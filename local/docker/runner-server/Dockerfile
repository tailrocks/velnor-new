FROM mcr.microsoft.com/dotnet/sdk:8.0@sha256:1b955f179b02df4f0eb5fadedaa898cbe44d620a64aab52eec72c8e91d1355b5 AS build
ARG RUNNER_SERVER_COMMIT=992ccbbbf9afcde477c38c316e053b1af457ad40
WORKDIR /src
RUN git init -q . \
    && git fetch -q --depth 1 https://github.com/ChristopherHX/runner.server "${RUNNER_SERVER_COMMIT}" \
    && git checkout -q FETCH_HEAD \
    && test "$(git rev-parse HEAD)" = "${RUNNER_SERVER_COMMIT}"
RUN sed -i 's|container.StoreName)), "application/octet-stream")|container.StoreName)), "application/zip")|' src/Runner.Server/Controllers/ArtifactControllerV2.cs \
    && grep -q 'container.StoreName)), "application/zip")' src/Runner.Server/Controllers/ArtifactControllerV2.cs
RUN sed -i \
      -e '/githubctx.Add("ref_type"/ s#Ref.StartsWith("refs/heads/") ? "branch"#Ref.StartsWith("refs/heads/") || Ref.StartsWith("refs/pull/") ? "branch"#' \
      -e '/githubctx.Add("ref_name"/ s#Ref.Substring("refs/heads/".Length) : ""#Ref.Substring("refs/heads/".Length) : Ref.StartsWith("refs/pull/") ? Ref.Substring("refs/pull/".Length) : ""#' \
      -e 's#public WebhookWorkflow Workflow  { get; set; }#[Newtonsoft.Json.JsonConverter(typeof(WorkflowObjectConverter))] public WebhookWorkflow Workflow  { get; set; }#' \
      src/Runner.Server/Controllers/MessageController.cs \
    && grep -q 'Ref.StartsWith("refs/pull/") ? "branch"' src/Runner.Server/Controllers/MessageController.cs \
    && grep -q 'Ref.StartsWith("refs/pull/") ? Ref.Substring("refs/pull/".Length)' src/Runner.Server/Controllers/MessageController.cs \
    && grep -q 'JsonConverter(typeof(WorkflowObjectConverter))\] public WebhookWorkflow Workflow' src/Runner.Server/Controllers/MessageController.cs
RUN sed -i \
      -e "s#elseif startsWith(server.ref, 'refs/heads/') }}:#elseif startsWith(server.ref, 'refs/heads/')||startsWith(server.ref,'refs/pull/') }}:#" \
      -e "s#ref_name: \${{ split(server.ref, 'refs/heads/')\[1\] }}#ref_name: \${{ split(server.ref, startsWith(server.ref, 'refs/heads/') \&\& 'refs/heads/' || 'refs/pull/')[1] }}#" \
      src/Runner.Server/contextTemplate.yml \
    && grep -q "||startsWith(server.ref,'refs/pull/') }}:" src/Runner.Server/contextTemplate.yml \
    && grep -q "|| 'refs/pull/')\[1\]" src/Runner.Server/contextTemplate.yml
COPY job-grant.cs.snippet /job-grant.cs.snippet
RUN test "$(grep -c 'if(!string.IsNullOrEmpty(github_token?.Value) ||' src/Runner.Server/Controllers/MessageController.cs)" = 1 \
    && sed -i '/if(!string.IsNullOrEmpty(github_token?.Value) ||/e cat /job-grant.cs.snippet' src/Runner.Server/Controllers/MessageController.cs \
    && grep -q 'new Claim("velnor", "job-cache-admin")' src/Runner.Server/Controllers/MessageController.cs
# runner.server arms a timer when it hands a job to a runner, so that the job's cancel request fires
# at timeout-minutes (360 when unset), and answers the runner's next poll with a cancel message, the
# way GitHub cancels a job that outruns its timeout. The wait on that poll never throws when the
# request fires (Helper.WaitAnyCancellationToken awaits Task.WhenAny, which completes without
# throwing), so the catch that sends the cancel message is dead code: the runner is never told, the
# job keeps running and the run waits for it for ever. Wait with Task.Delay, which throws, so the
# cancel message reaches the runner and runner.server completes the job as cancelled if the runner
# does not (cancel-timeout-minutes + 2). The job token keeps runner.server's own expiry, timeout-minutes
# + 10, which covers a cancelled job's reporting. Exactly one line is rewritten; the build fails when
# it moves.
RUN f=src/Runner.Server/Controllers/MessageController.cs \
    && test "$(grep -c 'await Helper.WaitAnyCancellationToken(jobRunningToken, ts.Token, job.CancelRequest.Token);' $f)" = 1 \
    && sed -i 's#await Helper.WaitAnyCancellationToken(jobRunningToken, ts.Token, job.CancelRequest.Token);#await Task.Delay(-1, CancellationTokenSource.CreateLinkedTokenSource(jobRunningToken, ts.Token, job.CancelRequest.Token).Token);#' $f \
    && grep -q 'await Task.Delay(-1, CancellationTokenSource.CreateLinkedTokenSource(jobRunningToken, ts.Token, job.CancelRequest.Token).Token);' $f \
    && ! grep -q 'WaitAnyCancellationToken(jobRunningToken, ts.Token, job.CancelRequest.Token)' $f
COPY WorkflowObjectConverter.cs src/Runner.Server/Controllers/WorkflowObjectConverter.cs
RUN cd src && dotnet msbuild dir.proj -t:GenerateConstant \
    && dotnet publish Runner.Client -c Release --no-self-contained -p:BUILD_OS=Any -p:RuntimeFrameworkVersion=8.0.0 -o /out

FROM mcr.microsoft.com/dotnet/aspnet:8.0@sha256:dccacae9e0e103b5d3ba5fbe442dd8d3316b990cd8f2ec9be0a0ec9db0c0e520
RUN apt-get update && apt-get install -y --no-install-recommends git && rm -rf /var/lib/apt/lists/*
COPY --from=build /out /opt/runner.server
WORKDIR /data
ENV ASPNETCORE_URLS=http://+:80
ENTRYPOINT ["dotnet", "/opt/runner.server/Runner.Server.dll"]
