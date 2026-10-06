From 857dce49347cbbe5da69c963e1adff718280dfbb Mon Sep 17 00:00:00 2001
From: Tauhid Anjum <tauhidanjum@gmail.com>
Date: Thu, 27 Oct 2022 11:35:45 +0000
Subject: [PATCH] Setting debug using GitHub Action variables

---
 src/Runner.Worker/ExecutionContext.cs    | 32 ++++++++-
 src/Test/L0/Worker/ExecutionContextL0.cs | 89 ++++++++++++++++++++++++
 2 files changed, 119 insertions(+), 2 deletions(-)

diff --git a/src/Runner.Worker/ExecutionContext.cs b/src/Runner.Worker/ExecutionContext.cs
index 1152f5845cf..aca3a105ca9 100644
--- a/src/Runner.Worker/ExecutionContext.cs
+++ b/src/Runner.Worker/ExecutionContext.cs
@@ -686,8 +686,11 @@ public void InitializeJob(Pipelines.AgentJobRequestMessage message, Cancellation
             // Endpoints
             Global.Endpoints = message.Resources.Endpoints;
 
-            // Variables
-            Global.Variables = new Variables(HostContext, message.Variables);
+            // Ser debug using vars context if debug variables are not already present.
+            var variables = message.Variables;
+            SetDebugUsingVars(variables, message.ContextData);
+
+            Global.Variables = new Variables(HostContext, variables);
 
             if (Global.Variables.GetBoolean("DistributedTask.ForceInternalNodeVersionOnRunnerTo12") ?? false)
             {
@@ -1077,6 +1080,31 @@ private IExecutionContext CreatePostChild(string displayName, Dictionary<string,
             return CreateChild(newGuid, displayName, newGuid.ToString("N"), null, null, ActionRunStage.Post, intraActionState, _childTimelineRecordOrder - Root.PostJobSteps.Count, siblingScopeName: siblingScopeName);
         }
 
+        // Sets debug using vars context in case debug variables are not present.
+        private static void SetDebugUsingVars(IDictionary<string, VariableValue> variables, IDictionary<string, PipelineContextData> contextData)
+        {
+            if (contextData != null &&
+                contextData.TryGetValue(PipelineTemplateConstants.Vars, out var varsPipelineContextData) &&
+                varsPipelineContextData != null && 
+                varsPipelineContextData is DictionaryContextData varsContextData)
+            {
+                // Set debug variables only when StepDebug/RunnerDebug variables are not present.
+                if (!variables.ContainsKey(Constants.Variables.Actions.StepDebug) &&
+                    varsContextData.TryGetValue(Constants.Variables.Actions.StepDebug, out var stepDebugValue) &&
+                    stepDebugValue is StringContextData)
+                {
+                    variables[Constants.Variables.Actions.StepDebug] = stepDebugValue.ToString();
+                }
+
+                if (!variables.ContainsKey(Constants.Variables.Actions.RunnerDebug) &&
+                    varsContextData.TryGetValue(Constants.Variables.Actions.RunnerDebug, out var runDebugValue) &&
+                    runDebugValue is StringContextData)
+                {
+                    variables[Constants.Variables.Actions.RunnerDebug] = runDebugValue.ToString();
+                }
+            }
+        }
+
         public void ApplyContinueOnError(TemplateToken continueOnErrorToken)
         {
             if (Result != TaskResult.Failed)
diff --git a/src/Test/L0/Worker/ExecutionContextL0.cs b/src/Test/L0/Worker/ExecutionContextL0.cs
index 670fece20c4..f70eee0417d 100644
--- a/src/Test/L0/Worker/ExecutionContextL0.cs
+++ b/src/Test/L0/Worker/ExecutionContextL0.cs
@@ -889,6 +889,95 @@ public void ActionVariables_AddedToVarsContext()
             }
         }
 
+        [Fact]
+        [Trait("Level", "L0")]
+        [Trait("Category", "Worker")]
+        public void ActionVariables_DebugUsingVars()
+        {
+            using (TestHostContext hc = CreateTestContext())
+            {
+                TaskOrchestrationPlanReference plan = new TaskOrchestrationPlanReference();
+                TimelineReference timeline = new TimelineReference();
+                Guid jobId = Guid.NewGuid();
+                string jobName = "some job name";
+                var jobRequest = new Pipelines.AgentJobRequestMessage(plan, timeline, jobId, jobName, jobName, null, null, null, new Dictionary<string, VariableValue>(), new List<MaskHint>(), new Pipelines.JobResources(), new Pipelines.ContextData.DictionaryContextData(), new Pipelines.WorkspaceOptions(), new List<Pipelines.ActionStep>(), null, null, null, null);
+                jobRequest.Resources.Repositories.Add(new Pipelines.RepositoryResource()
+                {
+                    Alias = Pipelines.PipelineConstants.SelfAlias,
+                    Id = "github",
+                    Version = "sha1"
+                });
+                jobRequest.ContextData["github"] = new Pipelines.ContextData.DictionaryContextData();
+
+                var inputVarsContext = new DictionaryContextData();
+
+                inputVarsContext[Constants.Variables.Actions.StepDebug] = new StringContextData("true");
+                inputVarsContext[Constants.Variables.Actions.RunnerDebug] = new StringContextData("true");
+                jobRequest.ContextData["vars"] = inputVarsContext;
+
+                // Arrange: Setup the paging logger. 
+                var pagingLogger1 = new Mock<IPagingLogger>();
+                var jobServerQueue = new Mock<IJobServerQueue>();
+                hc.EnqueueInstance(pagingLogger1.Object);
+                hc.SetSingleton(jobServerQueue.Object);
+
+                var jobContext = new Runner.Worker.ExecutionContext();
+                jobContext.Initialize(hc);
+
+                jobContext.InitializeJob(jobRequest, CancellationToken.None);
+
+                
+                Assert.Equal("true", jobContext.Global.Variables.Get(Constants.Variables.Actions.StepDebug));
+                Assert.Equal("true", jobContext.Global.Variables.Get(Constants.Variables.Actions.RunnerDebug));
+            }
+        }
+
+        [Fact]
+        [Trait("Level", "L0")]
+        [Trait("Category", "Worker")]
+        public void ActionVariables_SecretsPrecedenceForDebugUsingVars()
+        {
+            using (TestHostContext hc = CreateTestContext())
+            {
+                TaskOrchestrationPlanReference plan = new TaskOrchestrationPlanReference();
+                TimelineReference timeline = new TimelineReference();
+                Guid jobId = Guid.NewGuid();
+                string jobName = "some job name";
+                var jobRequest = new Pipelines.AgentJobRequestMessage(plan, timeline, jobId, jobName, jobName, null, null, null, new Dictionary<string, VariableValue>(), new List<MaskHint>(), new Pipelines.JobResources(), new Pipelines.ContextData.DictionaryContextData(), new Pipelines.WorkspaceOptions(), new List<Pipelines.ActionStep>(), null, null, null, null);
+                jobRequest.Resources.Repositories.Add(new Pipelines.RepositoryResource()
+                {
+                    Alias = Pipelines.PipelineConstants.SelfAlias,
+                    Id = "github",
+                    Version = "sha1"
+                });
+                jobRequest.ContextData["github"] = new Pipelines.ContextData.DictionaryContextData();
+
+                var inputVarsContext = new DictionaryContextData();
+
+                inputVarsContext[Constants.Variables.Actions.StepDebug] = new StringContextData("true");
+                inputVarsContext[Constants.Variables.Actions.RunnerDebug] = new StringContextData("true");
+                jobRequest.ContextData["vars"] = inputVarsContext;
+
+                jobRequest.Variables[Constants.Variables.Actions.StepDebug] = "false";
+                jobRequest.Variables[Constants.Variables.Actions.RunnerDebug] = "false";
+
+                // Arrange: Setup the paging logger. 
+                var pagingLogger1 = new Mock<IPagingLogger>();
+                var jobServerQueue = new Mock<IJobServerQueue>();
+                hc.EnqueueInstance(pagingLogger1.Object);
+                hc.SetSingleton(jobServerQueue.Object);
+
+                var jobContext = new Runner.Worker.ExecutionContext();
+                jobContext.Initialize(hc);
+
+                jobContext.InitializeJob(jobRequest, CancellationToken.None);
+
+                
+                Assert.Equal("false", jobContext.Global.Variables.Get(Constants.Variables.Actions.StepDebug));
+                Assert.Equal("false", jobContext.Global.Variables.Get(Constants.Variables.Actions.RunnerDebug));
+            }
+        }
+
         private bool ExpressionValuesAssertEqual(DictionaryContextData expect, DictionaryContextData actual)
         {
             foreach (var key in expect.Keys.ToList())
