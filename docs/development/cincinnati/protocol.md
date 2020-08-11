---
title: Cincinnati for Fedora CoreOS
parent: Development
nav_order: 3
layout: default
---

# Cincinnati for Fedora CoreOS

Cincinnati is a protocol to provide "update hints" to clients, and it builds upon experiences with the [Omaha update protocol][google-omaha].
It describes a particular method for representing transitions between releases of a project, allowing clients to apply updates in the right order.

[google-omaha]: https://github.com/google/omaha/blob/v1.3.33.7/doc/ServerProtocolV3.md

## Update Graph

Cincinnati uses a [directed acyclic graph][dag] (DAG) to represent the complete set of valid update-paths.
Each node in the graph is a release (with payload details) and each directed edge is a valid transition.

[dag]: https://en.wikipedia.org/wiki/Directed_acyclic_graph

## Clients

Cincinnati clients are the final consumers of the update graph and payloads.
A client periodically queries the Cincinnati service in order to fetch updates hints.
Once it discovers at least a valid update edge, it may or may not decide to apply it locally (based on its configuration and heuristic).

## Graph API

### Request

HTTP `GET` requests are used to fetch the DAG (as a JSON object) from the Graph API endpoint.
Requests SHOULD be sent to the Graph API endpoint at `/v1/graph` and MUST include the following header:

```
Accept: application/json
```

Fedora CoreOS clients MUST provide additional details as URL query parameters in the request.

|        Key       | Optional | Description                                           |
|------------------|----------|-------------------------------------------------------|
| basearch         | required | base architecture (non-empty string)                  |
| stream           | required | client-selected update stream (non-empty string)      |
| node_uuid        | optional | application-specific unique-identifier for the client |
| os_version       | optional | current OS version                                    |
| os_checksum      | optional | current OS checksum                                   |
| group            | optional | update group                                          |
| rollout_wariness | optional | client wariness to update rollout                     |
| platform         | optional | client platform                                       |

### Response

A positive response to the `/v1/graph` endpoint MUST be a JSON representation of the update graph.
Each known release is represented by an entry in the top-level `nodes` array.
Each of these entries includes the release version label, a payload identifier and any additional metadata. Each entry follows this schema:

|    Key   | Optional | Description                                                                             |
|----------|----------|-----------------------------------------------------------------------------------------|
| version  | required | the version of the release, as a unique (across "nodes" array) non-empty JSON string    |
| payload  | required | payload identifier, as a JSON string                                                    |
| metadata | required | a string-\>string map conveying arbitrary information about the release                 |

Allowed transitions between releases are represented as a top-level `edges` array, where each entry is an array-tuple.
Each of these tuples has two fields: the index of the starting node, and the index of the target node. Both are non-negative integers, ranging from 0 to `len(nodes)-1`.

For an example of a valid JSON document from a graph response, see [response.json](./response.json).

### Errors

Errors on the `/v1/graph` endpoint SHOULD be returned to the client as JSON objects, with a 4xx or 5xx HTTP status code.
Error values carry a type-identifier and a textual description, according to the following schema:

|  Key   | Optional | Description                                                  |
|--------|----------|--------------------------------------------------------------|
| kind   | required | error type identifier, as a non-empty JSON string            |
| value  | required | human-friendly error description, as a non-empty JSON string |

