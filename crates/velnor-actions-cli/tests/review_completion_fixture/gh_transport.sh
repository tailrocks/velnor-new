#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
state="$HOME/provider"
transport_error() { printf '%s\n' "$*" >> "$state/transport-errors"; exit 90; }
[ "$1" = api ] && [ "$2" = --include ] && [ "$3" = -H ] || transport_error invalid-api-invocation
[ "$4" = "Accept: application/vnd.github+json" ] && [ "$5" = -H ] && [ "$6" = "X-GitHub-Api-Version: 2026-03-10" ] || transport_error invalid-api-headers
shift 6
route=${1-}
shift
strip_nonce() {
 case "$route" in
  *\&_velnor_read_nonce=*) nonce=${route##*&_velnor_read_nonce=}; route=${route%&_velnor_read_nonce=$nonce} ;;
  *\?_velnor_read_nonce=*) nonce=${route##*\?_velnor_read_nonce=}; route=${route%?_velnor_read_nonce=$nonce} ;;
 esac
}
strip_nonce
case "$route" in
 *\?*) [ "${route#*\?}" = "per_page=100&page=1" ] || transport_error invalid-query ;;
esac
case "$route" in
 graphql) ;;
 *)
   case "${1-}" in
     --method) ;;
     *)
       [ "$#" -eq 4 ] && [ "$1" = -H ] && [ "$2" = "Cache-Control: no-cache, no-store" ] && [ "$3" = -H ] && [ "$4" = "Pragma: no-cache" ] || transport_error invalid-read-headers
       shift 4
       ;;
   esac
   ;;
esac
reply() { [ -r "$state/$2" ] || transport_error missing-fixture-response; printf 'HTTP/2 %s Fixture\r\ncontent-type: application/json\r\n\r\n' "$1"; cat "$state/$2"; }
case "$route" in
 repos/jackin-project/homebrew-tap) reply 200 repository.json ;;
 repos/jackin-project/homebrew-tap/pulls/514) reply 200 pull.json ;;
 repos/jackin-project/homebrew-tap/pulls/514/commits?*) reply 200 commits.json ;;
 repos/jackin-project/homebrew-tap/pulls/514/reviews?*) reply 200 reviews.json ;;
 repos/jackin-project/homebrew-tap/pulls/514/comments?*) reply 200 review-comments.json ;;
 repos/jackin-project/homebrew-tap/commits/*/comments?*) reply 200 empty-array.json ;;
 repos/jackin-project/homebrew-tap/issues/514/comments?*) reply 200 issue-comments.json ;;
 apps/chatgpt-codex-connector) reply 200 codex-app.json ;;
 apps/sonarqubecloud) reply 200 sonar-app.json ;;
 graphql)
   node=; query=; owner=; name=; number=; cursor=; node_count=0; query_count=0; owner_count=0; name_count=0; number_count=0; cursor_count=0
   while [ "$#" -gt 0 ]; do
     case "$1" in
       --raw-field) case "$2" in query=*) query=${2#query=}; query_count=$((query_count + 1)) ;; *) transport_error invalid-graphql-field ;; esac; shift 2 ;;
       --field)
         case "$2" in
           id=*) node=${2#id=}; node_count=$((node_count + 1)) ;;
           owner=*) owner=${2#owner=}; owner_count=$((owner_count + 1)) ;;
           name=*) name=${2#name=}; name_count=$((name_count + 1)) ;;
           number=*) number=${2#number=}; number_count=$((number_count + 1)) ;;
           cursor=*) cursor=${2#cursor=}; cursor_count=$((cursor_count + 1)) ;;
           *) transport_error invalid-graphql-variable ;;
         esac
         shift 2 ;;
       *) transport_error invalid-graphql-argument ;;
     esac
   done
   [ "$query_count" -eq 1 ] || transport_error invalid-graphql-shape
   case "$query" in
     'query($id:ID!){node(id:$id){... on IssueComment{databaseId body updatedAt lastEditedAt author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-editor-query-shape
       case "$node" in
         IC_codex_summary) reply 200 codex-editor.json ;;
         IC_sonar_summary) reply 200 sonar-editor.json ;;
         *) transport_error unmapped-graphql-node ;;
       esac ;;
     'query($id:ID!){node(id:$id){... on IssueComment{fullDatabaseId body updatedAt lastEditedAt author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-editor-query-shape
       case "$node" in
         IC_codex_summary) reply 200 codex-full-editor.json ;;
         IC_sonar_summary) reply 200 sonar-full-editor.json ;;
         *) transport_error unmapped-graphql-node ;;
       esac ;;
     'query($id:ID!){node(id:$id){... on PullRequestReview{databaseId body submittedAt lastEditedAt commit{oid} author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-review-editor-query-shape
       [ "$node" = PRR_codex_review ] || transport_error unmapped-graphql-node
       reply 200 review-editor.json ;;
     'query($id:ID!){node(id:$id){... on PullRequestReview{fullDatabaseId body submittedAt lastEditedAt commit{oid} author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-review-editor-query-shape
       case "$node" in
         PRR_codex_review) reply 200 review-full-editor.json ;;
         PRR_codex_followup) reply 200 review-followup-editor.json ;;
         *) transport_error unmapped-graphql-node ;;
       esac ;;
     'query($id:ID!){node(id:$id){... on PullRequestReview{fullDatabaseId body submittedAt lastEditedAt commit{oid} author{__typename ... on User{databaseId} ... on Bot{databaseId}} editor{__typename ... on User{databaseId} ... on Bot{databaseId}}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-user-review-editor-query-shape
       case "$node" in
         PRR_codex_review) reply 200 review-full-editor.json ;;
         PRR_codex_followup) reply 200 review-followup-editor.json ;;
         PRR_human_reply_0) reply 200 review-human-0.json ;;
         PRR_human_reply_1) reply 200 review-human-1.json ;;
         PRR_human_reply_orphan) reply 200 review-human-0.json ;;
         *) transport_error unmapped-graphql-node ;;
       esac ;;
     'query($id:ID!,$cursor:String){node(id:$id){... on PullRequestReviewThread{id isResolved isOutdated pullRequest{number repository{nameWithOwner}} comments(first:100,after:$cursor){nodes{id databaseId body updatedAt lastEditedAt author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}} pullRequestReview{databaseId}} pageInfo{hasNextPage endCursor}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-review-thread-query-shape
       [ "$node" = PRRT_codex_inline ] || transport_error unmapped-graphql-node
       reply 200 review-thread-syntax-error.json ;;
     'query($id:ID!,$cursor:String){node(id:$id){... on PullRequestReviewThread{id isResolved isOutdated pullRequest{number repository{nameWithOwner}} comments(first:100,after:$cursor){nodes{id fullDatabaseId body updatedAt lastEditedAt author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}} pullRequestReview{fullDatabaseId}} pageInfo{hasNextPage endCursor}}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-review-thread-query-shape
       [ "$node" = PRRT_codex_inline ] || transport_error unmapped-graphql-node
       reply 200 review-thread-full.json ;;
     'query($id:ID!,$cursor:String){node(id:$id){... on PullRequestReviewThread{id isResolved isOutdated pullRequest{number repository{nameWithOwner}} comments(first:100,after:$cursor){nodes{id fullDatabaseId body createdAt updatedAt lastEditedAt author{__typename ... on User{databaseId} ... on Bot{databaseId}} editor{__typename ... on User{databaseId} ... on Bot{databaseId}} pullRequestReview{fullDatabaseId} replyTo{id fullDatabaseId author{__typename ... on User{databaseId} ... on Bot{databaseId}}}} pageInfo{hasNextPage endCursor}}}}}')
       [ "$node_count" -eq 1 ] && [ "$owner_count" -eq 0 ] && [ "$name_count" -eq 0 ] && [ "$number_count" -eq 0 ] && [ "$cursor_count" -eq 0 ] || transport_error invalid-human-review-thread-query-shape
       [ "$node" = PRRT_codex_inline ] || transport_error unmapped-graphql-node
       if [ -f "$state/review-thread-human.json" ]; then
         reply 200 review-thread-human.json
       else
         reply 200 review-thread-expanded.json
       fi ;;
     'query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100,after:$cursor){nodes{id isResolved isOutdated} pageInfo{hasNextPage endCursor}}}}}')
       [ "$node_count" -eq 0 ] && [ "$owner_count" -eq 1 ] && [ "$name_count" -eq 1 ] && [ "$number_count" -eq 1 ] && [ "$cursor_count" -eq 0 ] && [ "$owner" = jackin-project ] && [ "$name" = homebrew-tap ] && [ "$number" = 514 ] && [ -z "$cursor" ] || transport_error invalid-threads-query-shape
       reply 200 threads.json ;;
     'query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){latestOpinionatedReviews(first:100,after:$cursor){nodes{id state submittedAt author{__typename ... on User{databaseId} ... on Bot{databaseId}}} pageInfo{hasNextPage endCursor}}}}}')
       [ "$node_count" -eq 0 ] && [ "$owner_count" -eq 1 ] && [ "$name_count" -eq 1 ] && [ "$number_count" -eq 1 ] && [ "$cursor_count" -eq 0 ] && [ "$owner" = jackin-project ] && [ "$name" = homebrew-tap ] && [ "$number" = 514 ] && [ -z "$cursor" ] || transport_error invalid-opinions-query-shape
       reply 200 opinions.json ;;
     *) transport_error unmapped-graphql-query ;;
   esac
   ;;
 repos/jackin-project/homebrew-tap/commits/*/check-runs?*)
   printf 'HTTP/2 200 Fixture\r\ncontent-type: application/json\r\n\r\n{"check_runs":['
   separator=
   if [ -f "$state/sonar-check.json" ]; then cat "$state/sonar-check.json"; separator=,; fi
   if [ -f "$state/completion-check.json" ]; then printf '%s' "$separator"; cat "$state/completion-check.json"; fi
   printf ']}' ;;
 repos/jackin-project/homebrew-tap/check-runs*)
   method=GET; name=; head=; status=; conclusion=; title=; summary=; text=; bad=0
   while [ "$#" -gt 0 ]; do
     case "$1" in
       --method) method="$2"; shift 2; continue ;;
       --raw-field)
         field="$2"; shift 2
         case "$field" in
           name=*) name=${field#*=} ;;
           head_sha=*) head=${field#*=} ;;
           status=*) status=${field#*=} ;;
           conclusion=*) conclusion=${field#*=} ;;
           output\[title\]=*) title=${field#*=} ;;
           output\[summary\]=*) summary=${field#*=} ;;
           output\[text\]=*) text=${field#*=} ;;
           *) bad=1 ;;
         esac
         continue ;;
       *) bad=1; shift ;;
     esac
   done
   [ "$bad" -eq 0 ] || transport_error invalid-check-request
   case "$route:$method" in
     repos/jackin-project/homebrew-tap/check-runs:POST|repos/jackin-project/homebrew-tap/check-runs/*:PATCH|repos/jackin-project/homebrew-tap/check-runs/*:GET) ;;
     *) transport_error invalid-check-method ;;
   esac
   if [ "$method" = GET ]; then reply 200 completion-check.json; exit $?; fi
   if [ "$method" = POST ] && [ -f "$state/mutate-on-check-post" ]; then
     mv "$state/issue-comments-after-capture.json" "$state/issue-comments.json" || transport_error post-capture-fixture-change
     rm "$state/mutate-on-check-post" || transport_error post-capture-fixture-change
   fi
   if [ "$method" = PATCH ]; then head=$(cat "$state/completion-head"); fi
   [ -n "$head" ] && [ -n "$name" ] && [ -n "$status" ] && [ -n "$conclusion" ] || transport_error missing-check-fields
   printf '%s' "$head" > "$state/completion-head"
   printf '%s' "$name" > "$state/check-name"
   printf '%s' "$status" > "$state/check-status"
   printf '%s' "$conclusion" > "$state/check-conclusion"
   printf '%s' "$title" > "$state/check-title"
   printf '%s' "$summary" > "$state/check-summary"
   printf '%s' "$text" > "$state/completion-text.json"
   json_string() {
     printf '"'
     od -An -v -tu1 "$1" | tr -s ' ' '\n' | while IFS= read -r octet; do
       [ -n "$octet" ] || continue
       case "$octet" in
         8) printf '\\b' ;;
         9) printf '\\t' ;;
         10) printf '\\n' ;;
         12) printf '\\f' ;;
         13) printf '\\r' ;;
         34) printf '\\"' ;;
         92) printf '\\\\' ;;
         0|1|2|3|4|5|6|7|11|14|15|16|17|18|19|20|21|22|23|24|25|26|27|28|29|30|31) printf '\\u%04x' "$octet" ;;
         *) printf '%b' "\\$(printf '%03o' "$octet")" ;;
       esac
     done
     printf '"'
   }
   qname=$(json_string "$state/check-name")
   qhead=$(json_string "$state/completion-head")
   qstatus=$(json_string "$state/check-status")
   qconclusion=$(json_string "$state/check-conclusion")
   qtitle=$(json_string "$state/check-title")
   qsummary=$(json_string "$state/check-summary")
   qtext=$(json_string "$state/completion-text.json")
   printf '{"id":122300000001,"name":%s,"head_sha":%s,"status":%s,"conclusion":%s,"output":{"title":%s,"summary":%s,"text":%s},"app":{"id":5399768},"pull_requests":[{"number":514,"head":{"sha":%s},"base":{"repo":{"id":1200197694}}}]}' "$qname" "$qhead" "$qstatus" "$qconclusion" "$qtitle" "$qsummary" "$qtext" "$qhead" > "$state/completion-check.json"
   if [ "$method" = POST ]; then reply 201 completion-check.json; else reply 200 completion-check.json; fi ;;
 *) transport_error unmapped-route ;;
esac
