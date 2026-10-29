-- Redacted synthetic opencode 1.18.35 repair-pattern subset; tables: project, session, message, part.
CREATE TABLE `message` (
          `id` text PRIMARY KEY,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
        );
CREATE TABLE `part` (
          `id` text PRIMARY KEY,
          `message_id` text NOT NULL,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_part_message_id_message_id_fk` FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON DELETE CASCADE
        );
CREATE TABLE `project` (
          `id` text PRIMARY KEY,
          `worktree` text NOT NULL,
          `vcs` text,
          `name` text,
          `icon_url` text,
          `icon_url_override` text,
          `icon_color` text,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `time_initialized` integer,
          `sandboxes` text NOT NULL,
          `commands` text
        );
INSERT INTO "project" VALUES('proj_example','/home/user/project','git',NULL,NULL,NULL,NULL,1791436986112,1791436987676,NULL,'[]',NULL);
INSERT INTO "project" VALUES('global','/',NULL,NULL,NULL,NULL,NULL,1791427585862,1793196095498,NULL,'[]',NULL);
CREATE TABLE `session` (
          `id` text PRIMARY KEY,
          `project_id` text NOT NULL,
          `workspace_id` text,
          `parent_id` text,
          `slug` text NOT NULL,
          `directory` text NOT NULL,
          `path` text,
          `title` text NOT NULL,
          `version` text NOT NULL,
          `share_url` text,
          `summary_additions` integer,
          `summary_deletions` integer,
          `summary_files` integer,
          `summary_diffs` text,
          `metadata` text,
          `cost` real DEFAULT 0 NOT NULL,
          `tokens_input` integer DEFAULT 0 NOT NULL,
          `tokens_output` integer DEFAULT 0 NOT NULL,
          `tokens_reasoning` integer DEFAULT 0 NOT NULL,
          `tokens_cache_read` integer DEFAULT 0 NOT NULL,
          `tokens_cache_write` integer DEFAULT 0 NOT NULL,
          `revert` text,
          `permission` text,
          `agent` text,
          `model` text,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `time_compacting` integer,
          `time_archived` integer,
          CONSTRAINT `fk_session_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE
        );
INSERT INTO "session" VALUES('ses_example1','proj_example',NULL,NULL,'synthetic','/home/user/project',NULL,'Synthetic session','1.18.35',NULL,NULL,NULL,NULL,NULL,NULL,0.0065606339999999992,38995,635,182,73728,0,NULL,NULL,NULL,NULL,1791436988255,1791437100900,NULL,NULL);

INSERT INTO "message" VALUES('msg_0001','ses_example1',1791436988425,1791436988425,'{"role":"user","time":{"created":1791436988425},"agent":"build","model":{"providerID":"example","modelID":"example-model-a"},"summary":{"diffs":[]}}');
INSERT INTO "message" VALUES('msg_0002','ses_example1',1791436989373,1791436989373,'{"parentID":"msg_0001","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0,"tokens":{"total":100,"input":80,"output":20,"reasoning":0,"cache":{"write":0,"read":0}},"modelID":"example-model-a","providerID":"example","time":{"created":1791436989373,"completed":1791436996048},"finish":"stop"}');
INSERT INTO "part" VALUES('prt_0001','msg_0001','ses_example1',1791436989348,1791436989348,'{"type":"text","text":"Run the tests."}');
INSERT INTO "part" VALUES('prt_0002','msg_0002','ses_example1',1791436995532,1791436995532,'{"type":"tool","tool":"bash","callID":"call_001","state":{"status":"completed","input":{"command":"python -m pytest tests/test_a.py -q"},"output":"python: No module named pytest","metadata":{"output":"python: No module named pytest","exit":1,"truncated":false},"title":"synthetic","time":{"start":1791436995532,"end":1791436995632}}}');
INSERT INTO "part" VALUES('prt_0003','msg_0002','ses_example1',1791436995832,1791436995832,'{"type":"tool","tool":"bash","callID":"call_002","state":{"status":"completed","input":{"command":".venv/proj/bin/python -m pytest tests/test_a.py -q"},"output":"1 passed in 0.2s","metadata":{"output":"1 passed in 0.2s","exit":0,"truncated":false},"title":"synthetic","time":{"start":1791436995832,"end":1791436995932}}}');