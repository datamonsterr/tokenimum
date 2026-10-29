-- Redacted synthetic opencode 1.18.35 database subset; tables: project, session, message, part.
CREATE TABLE `message` (
          `id` text PRIMARY KEY,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
        );
INSERT INTO "message" VALUES('msg_0001','ses_example1',1791436988425,1791436999372,'{"role":"user","time":{"created":1791436988425},"agent":"build","model":{"providerID":"example","modelID":"example-model-a"},"summary":{"diffs":[]}}');
INSERT INTO "message" VALUES('msg_0002','ses_example1',1791436989373,1791436996048,'{"parentID":"msg_0001","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0.005592,"tokens":{"total":36683,"input":36484,"output":115,"reasoning":84,"cache":{"write":0,"read":0}},"modelID":"example-model-a","providerID":"example","time":{"created":1791436989373,"completed":1791436996047},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0003','ses_example1',1791436996051,1791436999169,'{"parentID":"msg_0001","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0.00038529,"tokens":{"total":37506,"input":755,"output":264,"reasoning":7,"cache":{"write":0,"read":36480}},"modelID":"example-model-a","providerID":"example","time":{"created":1791436996051,"completed":1791436999168},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0004','ses_example1',1791436999174,1791437002447,'{"parentID":"msg_0001","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0.000583344,"tokens":{"total":39351,"input":1756,"output":256,"reasoning":91,"cache":{"write":0,"read":37248}},"modelID":"example-model-a","providerID":"example","time":{"created":1791436999174,"completed":1791437002447},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0005','ses_example2',1792131725233,1792131747615,'{"role":"user","time":{"created":1792131725233},"agent":"build","model":{"providerID":"example","modelID":"example-model-b"},"summary":{"diffs":[]}}');
INSERT INTO "message" VALUES('msg_0006','ses_example2',1792131725272,1792131732320,'{"parentID":"msg_0005","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0,"tokens":{"total":35524,"input":34600,"output":85,"reasoning":598,"cache":{"write":0,"read":241}},"modelID":"example-model-b","providerID":"example","time":{"created":1792131725272,"completed":1792131732318},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0007','ses_example2',1792131732324,1792131735576,'{"parentID":"msg_0005","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0,"tokens":{"total":35863,"input":789,"output":113,"reasoning":160,"cache":{"write":0,"read":34801}},"modelID":"example-model-b","providerID":"example","time":{"created":1792131732324,"completed":1792131735575},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0008','ses_example2',1792131735579,1792131739472,'{"parentID":"msg_0005","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0,"tokens":{"total":36397,"input":373,"output":160,"reasoning":295,"cache":{"write":0,"read":35569}},"modelID":"example-model-b","providerID":"example","time":{"created":1792131735579,"completed":1792131739471},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0009','ses_example2',1792131739474,1792131741221,'{"parentID":"msg_0005","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0,"tokens":{"total":36585,"input":700,"output":54,"reasoning":6,"cache":{"write":0,"read":35825}},"modelID":"example-model-b","providerID":"example","time":{"created":1792131739474,"completed":1792131741220},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0010','ses_example2',1792131741224,1792131745077,'{"parentID":"msg_0005","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0,"tokens":{"total":37161,"input":510,"output":131,"reasoning":55,"cache":{"write":0,"read":36465}},"modelID":"example-model-b","providerID":"example","time":{"created":1792131741224,"completed":1792131745075},"finish":"tool-calls"}');
INSERT INTO "message" VALUES('msg_0011','ses_example2',1792131745081,1792131747592,'{"parentID":"msg_0005","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0,"tokens":{"total":38351,"input":1111,"output":207,"reasoning":184,"cache":{"write":0,"read":36849}},"modelID":"example-model-b","providerID":"example","time":{"created":1792131745081,"completed":1792131747590},"finish":"stop"}');
INSERT INTO "message" VALUES('msg_9001','ses_example1',1791437100000,1791437100000,'{"role":"user","time":{"created":1791437100000},"agent":"build","model":{"providerID":"example","modelID":"example-model-a"},"summary":{"diffs":[]}}');
INSERT INTO "message" VALUES('msg_9002','ses_example1',1791437100010,1791437100010,'{"parentID":"msg_9001","role":"assistant","mode":"build","agent":"build","path":{"cwd":"/home/user/project","root":"/home/user/project"},"cost":0.01,"tokens":{"total":1000,"input":100,"output":20,"reasoning":5,"cache":{"write":7,"read":300}},"modelID":"example-model-a","providerID":"example","time":{"created":1791437100010,"completed":1791437100900},"finish":"stop"}');
CREATE TABLE `part` (
          `id` text PRIMARY KEY,
          `message_id` text NOT NULL,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_part_message_id_message_id_fk` FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON DELETE CASCADE
        );
INSERT INTO "part" VALUES('prt_0001','msg_0001','ses_example1',1791436989348,1791436989350,'{"type":"text","text":"Synthetic prompt."}');
INSERT INTO "part" VALUES('prt_0002','msg_0002','ses_example1',1791436994101,1791436994104,'{"snapshot":"0000000000000000000000000000000000000001","type":"step-start"}');
INSERT INTO "part" VALUES('prt_0003','msg_0002','ses_example1',1791436995023,1791436995529,'{"type":"reasoning","text":"synthetic","time":{"start":1791436995022,"end":1791436995528}}');
INSERT INTO "part" VALUES('prt_0004','msg_0002','ses_example1',1791436995532,1791436995944,'{"type":"tool","tool":"bash","callID":"call_004","state":{"status":"completed","input":{"command":"synthetic-command"},"output":"synthetic","metadata":{"output":"synthetic","exit":0,"truncated":false},"title":"synthetic","time":{"start":1791436995669,"end":1791436995943}}}');
INSERT INTO "part" VALUES('prt_0005','msg_0002','ses_example1',1791436995673,1791436995918,'{"type":"tool","tool":"bash","callID":"call_005","state":{"status":"completed","input":{"command":"synthetic-command"},"output":"synthetic","metadata":{"output":"synthetic","exit":0,"truncated":false},"title":"synthetic","time":{"start":1791436995783,"end":1791436995917}}}');
INSERT INTO "part" VALUES('prt_0006','msg_0002','ses_example1',1791436995996,1791436995997,'{"reason":"tool-calls","snapshot":"0000000000000000000000000000000000000001","type":"step-finish","tokens":{"total":36683,"input":36484,"output":115,"reasoning":84,"cache":{"write":0,"read":0}},"cost":0.005592}');
INSERT INTO "part" VALUES('prt_0007','msg_0003','ses_example1',1791436997719,1791436997720,'{"snapshot":"0000000000000000000000000000000000000001","type":"step-start"}');
INSERT INTO "part" VALUES('prt_0008','msg_0003','ses_example1',1791436998202,1791436998362,'{"type":"reasoning","text":"synthetic","time":{"start":1791436998202,"end":1791436998361}}');
INSERT INTO "part" VALUES('prt_0009','msg_0003','ses_example1',1791436998364,1791436998509,'{"type":"tool","tool":"read","callID":"call_009","state":{"status":"completed","input":{"filePath":"/home/user/project/file.txt"},"output":"synthetic","metadata":{"preview":"synthetic","truncated":false,"loaded":[],"display":{"type":"file","path":"/home/user/project","text":"synthetic","lineStart":1,"lineEnd":13,"totalLines":13,"truncated":false}},"title":"synthetic","time":{"start":1791436998486,"end":1791436998508}}}');
INSERT INTO "part" VALUES('prt_0010','msg_0003','ses_example1',1791436998557,1791436998694,'{"type":"tool","tool":"read","callID":"call_010","state":{"status":"completed","input":{"filePath":"/home/user/project/file.txt"},"output":"synthetic","metadata":{"preview":"synthetic","truncated":false,"loaded":[],"display":{"type":"file","path":"/home/user/project","text":"synthetic","lineStart":1,"lineEnd":33,"totalLines":33,"truncated":false}},"title":"synthetic","time":{"start":1791436998617,"end":1791436998693}}}');
INSERT INTO "part" VALUES('prt_0011','msg_0003','ses_example1',1791436998636,1791436998858,'{"type":"tool","tool":"read","callID":"call_011","state":{"status":"completed","input":{"filePath":"/home/user/project/file.txt"},"output":"synthetic","metadata":{"preview":"synthetic","truncated":false,"loaded":[],"display":{"type":"file","path":"/home/user/project","text":"synthetic","lineStart":1,"lineEnd":4,"totalLines":4,"truncated":false}},"title":"synthetic","time":{"start":1791436998748,"end":1791436998857}}}');
INSERT INTO "part" VALUES('prt_0012','msg_0003','ses_example1',1791436998769,1791436999052,'{"type":"tool","tool":"bash","callID":"call_012","state":{"status":"completed","input":{"command":"synthetic-command"},"output":"synthetic","metadata":{"output":"synthetic","exit":0,"truncated":false},"title":"synthetic","time":{"start":1791436998965,"end":1791436999050}}}');
INSERT INTO "part" VALUES('prt_0013','msg_0003','ses_example1',1791436999103,1791436999105,'{"reason":"tool-calls","snapshot":"0000000000000000000000000000000000000001","type":"step-finish","tokens":{"total":37506,"input":755,"output":264,"reasoning":7,"cache":{"write":0,"read":36480}},"cost":0.00038529}');
INSERT INTO "part" VALUES('prt_0014','msg_0004','ses_example1',1791437000855,1791437000856,'{"snapshot":"0000000000000000000000000000000000000001","type":"step-start"}');
INSERT INTO "part" VALUES('prt_0015','msg_0004','ses_example1',1791437001300,1791437001868,'{"type":"reasoning","text":"synthetic","time":{"start":1791437001299,"end":1791437001866}}');
INSERT INTO "part" VALUES('prt_0016','msg_0004','ses_example1',1791437001869,1791437002094,'{"type":"tool","tool":"bash","callID":"call_016","state":{"status":"error","input":{"command":"synthetic-command","workdir":"synthetic"},"error":"synthetic","time":{"start":1791437002028,"end":1791437002093}}}');
INSERT INTO "part" VALUES('prt_0017','msg_0004','ses_example1',1791437002054,1791437002199,'{"type":"tool","tool":"read","callID":"call_017","state":{"status":"completed","input":{"filePath":"/home/user/project/file.txt"},"output":"synthetic","metadata":{"preview":"synthetic","truncated":false,"loaded":[],"display":{"type":"file","path":"/home/user/project","text":"synthetic","lineStart":1,"lineEnd":70,"totalLines":70,"truncated":false}},"title":"synthetic","time":{"start":1791437002178,"end":1791437002198}}}');
INSERT INTO "part" VALUES('prt_0018','msg_0004','ses_example1',1791437002193,1791437002339,'{"type":"tool","tool":"read","callID":"call_018","state":{"status":"completed","input":{"filePath":"/home/user/project/file.txt"},"output":"synthetic","metadata":{"preview":"synthetic","truncated":false,"loaded":[],"display":{"type":"file","path":"/home/user/project","text":"synthetic","lineStart":1,"lineEnd":614,"totalLines":614,"truncated":false}},"title":"synthetic","time":{"start":1791437002319,"end":1791437002338}}}');
INSERT INTO "part" VALUES('prt_0019','msg_0004','ses_example1',1791437002396,1791437002397,'{"reason":"tool-calls","snapshot":"0000000000000000000000000000000000000001","type":"step-finish","tokens":{"total":39351,"input":1756,"output":256,"reasoning":91,"cache":{"write":0,"read":37248}},"cost":0.000583344}');
INSERT INTO "part" VALUES('prt_0020','msg_0005','ses_example2',1792131725250,1792131725252,'{"type":"text","text":"Synthetic prompt."}');
INSERT INTO "part" VALUES('prt_0021','msg_0006','ses_example2',1792131728105,1792131728107,'{"type":"step-start"}');
INSERT INTO "part" VALUES('prt_0022','msg_0006','ses_example2',1792131728109,1792131731662,'{"type":"reasoning","text":"synthetic","time":{"start":1792131728109,"end":1792131731659},"metadata":{"openai":{"itemId":"synthetic","reasoningEncryptedContent":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0023','msg_0006','ses_example2',1792131731829,1792131731844,'{"type":"text","text":"Synthetic answer.","time":{"start":1792131731829,"end":1792131731842},"metadata":{"openai":{"itemId":"rs_01a14360104c76a7a4c4ef765f18f075","phase":"commentary"}}}');
INSERT INTO "part" VALUES('prt_0024','msg_0006','ses_example2',1792131731859,1792131732303,'{"type":"tool","tool":"bash","callID":"call_024","state":{"status":"completed","input":{"command":"synthetic-command"},"output":"synthetic","metadata":{"output":"synthetic","exit":0,"truncated":false},"title":"synthetic","time":{"start":1792131732139,"end":1792131732301}},"metadata":{"openai":{"itemId":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0025','msg_0006','ses_example2',1792131732308,1792131732310,'{"reason":"tool-calls","type":"step-finish","tokens":{"total":35524,"input":34600,"output":85,"reasoning":598,"cache":{"write":0,"read":241}},"cost":0}');
INSERT INTO "part" VALUES('prt_0026','msg_0007','ses_example2',1792131734955,1792131734958,'{"type":"step-start"}');
INSERT INTO "part" VALUES('prt_0027','msg_0007','ses_example2',1792131734961,1792131734965,'{"type":"reasoning","text":"synthetic","time":{"start":1792131734961,"end":1792131734964},"metadata":{"openai":{"itemId":"synthetic","reasoningEncryptedContent":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0028','msg_0007','ses_example2',1792131734966,1792131735566,'{"type":"tool","tool":"bash","callID":"call_028","state":{"status":"completed","input":{"command":"synthetic-command","timeout":20000},"output":"synthetic","metadata":{"output":"synthetic","exit":0,"truncated":false},"title":"synthetic","time":{"start":1792131734973,"end":1792131735565}},"metadata":{"openai":{"itemId":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0029','msg_0007','ses_example2',1792131735568,1792131735570,'{"reason":"tool-calls","type":"step-finish","tokens":{"total":35863,"input":789,"output":113,"reasoning":160,"cache":{"write":0,"read":34801}},"cost":0}');
INSERT INTO "part" VALUES('prt_0030','msg_0008','ses_example2',1792131737349,1792131737351,'{"type":"step-start"}');
INSERT INTO "part" VALUES('prt_0031','msg_0008','ses_example2',1792131737352,1792131738412,'{"type":"reasoning","text":"synthetic","time":{"start":1792131737352,"end":1792131738409},"metadata":{"openai":{"itemId":"synthetic","reasoningEncryptedContent":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0032','msg_0008','ses_example2',1792131738489,1792131738925,'{"type":"tool","tool":"read","callID":"call_032","state":{"status":"completed","input":{"filePath":"/home/user/project/file.txt"},"output":"synthetic","metadata":{"preview":"synthetic","truncated":false,"loaded":[],"display":{"type":"directory","path":"/home/user/project","entries":["synthetic","synthetic","synthetic","synthetic","synthetic","synthetic","synthetic","synthetic"],"offset":1,"totalEntries":8,"truncated":false}},"title":"synthetic","time":{"start":1792131738887,"end":1792131738924}},"metadata":{"openai":{"itemId":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0033','msg_0008','ses_example2',1792131738593,1792131739463,'{"type":"tool","tool":"bash","callID":"call_033","state":{"status":"completed","input":{"command":"synthetic-command","timeout":20000},"output":"synthetic","metadata":{"output":"synthetic","exit":0,"truncated":false},"title":"synthetic","time":{"start":1792131738896,"end":1792131739462}},"metadata":{"openai":{"itemId":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0034','msg_0008','ses_example2',1792131739465,1792131739466,'{"reason":"tool-calls","type":"step-finish","tokens":{"total":36397,"input":373,"output":160,"reasoning":295,"cache":{"write":0,"read":35569}},"cost":0}');
INSERT INTO "part" VALUES('prt_0035','msg_0009','ses_example2',1792131741153,1792131741155,'{"type":"step-start"}');
INSERT INTO "part" VALUES('prt_0036','msg_0009','ses_example2',1792131741158,1792131741163,'{"type":"reasoning","text":"synthetic","time":{"start":1792131741158,"end":1792131741161},"metadata":{"openai":{"itemId":"synthetic","reasoningEncryptedContent":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0037','msg_0009','ses_example2',1792131741166,1792131741198,'{"type":"tool","tool":"read","callID":"call_037","state":{"status":"completed","input":{"filePath":"/home/user/project/file.txt"},"output":"synthetic","metadata":{"preview":"synthetic","truncated":false,"loaded":[],"display":{"type":"file","path":"/home/user/project","text":"synthetic","lineStart":1,"lineEnd":28,"totalLines":28,"truncated":false}},"title":"synthetic","time":{"start":1792131741172,"end":1792131741196}},"metadata":{"openai":{"itemId":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0038','msg_0009','ses_example2',1792131741216,1792131741217,'{"reason":"tool-calls","type":"step-finish","tokens":{"total":36585,"input":700,"output":54,"reasoning":6,"cache":{"write":0,"read":35825}},"cost":0}');
INSERT INTO "part" VALUES('prt_0039','msg_0010','ses_example2',1792131744155,1792131744158,'{"type":"step-start"}');
INSERT INTO "part" VALUES('prt_0040','msg_0010','ses_example2',1792131744162,1792131744167,'{"type":"reasoning","text":"synthetic","time":{"start":1792131744161,"end":1792131744166},"metadata":{"openai":{"itemId":"synthetic","reasoningEncryptedContent":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0041','msg_0010','ses_example2',1792131744168,1792131745061,'{"type":"tool","tool":"bash","callID":"call_041","state":{"status":"completed","input":{"command":"synthetic-command","timeout":20000},"output":"synthetic","metadata":{"output":"synthetic","exit":0,"truncated":false},"title":"synthetic","time":{"start":1792131744477,"end":1792131745059}},"metadata":{"openai":{"itemId":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0042','msg_0010','ses_example2',1792131745065,1792131745067,'{"reason":"tool-calls","type":"step-finish","tokens":{"total":37161,"input":510,"output":131,"reasoning":55,"cache":{"write":0,"read":36465}},"cost":0}');
INSERT INTO "part" VALUES('prt_0043','msg_0011','ses_example2',1792131747028,1792131747031,'{"type":"step-start"}');
INSERT INTO "part" VALUES('prt_0044','msg_0011','ses_example2',1792131747035,1792131747040,'{"type":"reasoning","text":"synthetic","time":{"start":1792131747035,"end":1792131747038},"metadata":{"openai":{"itemId":"synthetic","reasoningEncryptedContent":"synthetic"}}}');
INSERT INTO "part" VALUES('prt_0045','msg_0011','ses_example2',1792131747042,1792131747527,'{"type":"text","text":"Synthetic answer.","time":{"start":1792131747042,"end":1792131747526},"metadata":{"openai":{"itemId":"msg_01a143604b9876788505307973770f7a"}}}');
INSERT INTO "part" VALUES('prt_0046','msg_0011','ses_example2',1792131747579,1792131747581,'{"reason":"stop","type":"step-finish","tokens":{"total":38351,"input":1111,"output":207,"reasoning":184,"cache":{"write":0,"read":36849}},"cost":0}');
INSERT INTO "part" VALUES('prt_9001','msg_9001','ses_example1',1791437100001,1791437100001,'{"type":"text","text":"Synthetic follow-up prompt."}');
INSERT INTO "part" VALUES('prt_9002','msg_9001','ses_example1',1791437100002,1791437100002,'{"type":"text","text":"Synthetic injected context.","synthetic":true}');
INSERT INTO "part" VALUES('prt_9003','msg_9002','ses_example1',1791437100011,1791437100011,'{"type":"step-start","snapshot":"0000000000000000000000000000000000000002"}');
INSERT INTO "part" VALUES('prt_9004','msg_9002','ses_example1',1791437100012,1791437100012,'{"type":"tool","tool":"write","callID":"call_901","state":{"status":"completed","input":{"filePath":"/home/user/project/src/new.rs","content":"synthetic"},"output":"Wrote file successfully.","title":"src/new.rs","metadata":{"filepath":"/home/user/project/src/new.rs","exists":false},"time":{"start":1791437100100,"end":1791437100150}}}');
INSERT INTO "part" VALUES('prt_9005','msg_9002','ses_example1',1791437100013,1791437100013,'{"type":"tool","tool":"edit","callID":"call_902","state":{"status":"completed","input":{"filePath":"/home/user/project/src/lib.rs","oldString":"a","newString":"b"},"output":"Edit applied successfully.","title":"src/lib.rs","metadata":{"diff":"synthetic"},"time":{"start":1791437100200,"end":1791437100260}}}');
INSERT INTO "part" VALUES('prt_9006','msg_9002','ses_example1',1791437100014,1791437100014,'{"type":"tool","tool":"edit","callID":"call_903","state":{"status":"completed","input":{"filePath":"/etc/hosts","oldString":"a","newString":"b"},"output":"Edit applied successfully.","title":"hosts","metadata":{},"time":{"start":1791437100300,"end":1791437100310}}}');
INSERT INTO "part" VALUES('prt_9007','msg_9002','ses_example1',1791437100015,1791437100015,'{"type":"tool","tool":"bash","callID":"call_904","state":{"status":"error","input":{"command":"synthetic-failing-command"},"error":"Error: command failed","metadata":{"exit":2},"time":{"start":1791437100400,"end":1791437100450}}}');
INSERT INTO "part" VALUES('prt_9008','msg_9002','ses_example1',1791437100016,1791437100016,'{"type":"tool","tool":"bash","callID":"call_905","state":{"status":"completed","input":{"command":"synthetic-nonzero"},"output":"partial output\nExit status 1","title":"synthetic","metadata":{"output":"partial output","exit":1,"truncated":false},"time":{"start":1791437100500,"end":1791437100520}}}');
INSERT INTO "part" VALUES('prt_9009','msg_9002','ses_example1',1791437100017,1791437100017,'{"type":"tool","tool":"example_server_tool","callID":"call_906","state":{"status":"completed","input":{"q":"synthetic"},"output":"ok","title":"t","metadata":{},"time":{"start":1791437100600,"end":1791437100601}}}');
INSERT INTO "part" VALUES('prt_9010','msg_9002','ses_example1',1791437100018,1791437100018,'{"type":"patch","hash":"0000000000000000000000000000000000000003","files":["/home/user/project/src/new.rs"]}');
INSERT INTO "part" VALUES('prt_9011','msg_9002','ses_example1',1791437100019,1791437100019,'{"type":"future-widget","payload":"synthetic"}');
INSERT INTO "part" VALUES('prt_9012','msg_9002','ses_example1',1791437100020,1791437100020,'{"type":"step-finish","reason":"stop","snapshot":"0000000000000000000000000000000000000004","tokens":{"total":1000,"input":100,"output":20,"reasoning":5,"cache":{"write":7,"read":300}},"cost":0.01}');
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
INSERT INTO "session" VALUES('ses_example2','proj_example',NULL,NULL,'synthetic','/home/user/project',NULL,'Synthetic session','1.18.35',NULL,NULL,NULL,NULL,NULL,NULL,0.0,38083,750,1298,179750,0,NULL,NULL,NULL,NULL,1792131725177,1792131747600,NULL,NULL);
