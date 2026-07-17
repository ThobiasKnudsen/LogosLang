# Follow Elon Musks rules faithfully:
1. **Question every requirement:** Find out who exactly created the requirement and never just accept that it's from "the safety or legal department". Make the requirements less dumb.
2. **Delete parts or processes:** Delete as many parts or steps as you can. Musk's rule of thumb: If you aren't forced to put back at least 10% of what you deleted, you didn't delete enough.
3. **Simplify and optimize:** Only do this after you have completed steps 1 and 2, because a massive waste of time is optimizing something that shouldn't exist in the first place.
4. **Accelerate cycle time:** Speed up the process.
5. **Automate:** Only automate the process after the first four steps are done.

# Faithfulness protocol (spec-governed code):
- DESIGN.md is the ruling document. language_sketch.logos illustrates it; issues, plans, memories, old comments, and existing code are downstream and may be stale. Never implement from a downstream source alone.
- Before implementing anything spec-governed, quote the exact DESIGN.md passage(s) that license it, in the plan or the commit message. No quote → stop and ask.
- If any two sources disagree — DESIGN vs sketch, DESIGN vs an issue, one DESIGN section vs another — STOP and surface the conflict as a blocking question, with both quotes. Never silently pick a side, even if one side is newer or was written by Thobias: staleness is invisible from inside a session.
- When a conflict is ruled on, or a design is rejected in conversation, record it in the same session: in DESIGN.md (the existing pattern: "Recorded as rejected, to stay rejected: …") or, if spec wording must wait, as an explicit pending-spec-edit in the session log AND auto-memory. An unrecorded decision is a future bug.
- Before starting work in a spec area not touched recently, run /faithfulness-audit.

# Follow logging rules faithfully:
- At session start create a file under CLAUDE_LOG folder with the name Session_YYYY-MM-DD_HH:mm:ss.md. 
- What should be said in the start of the file is the session id so that Claude can find the actual session later for more details. Format like this «# Session id: [session id]»
- At the end of each response for the user request you always append to the file, starting with a heading on this format «## response time: YYYY-MM-DD_HH:mm:ss | LLM: [LLM model responding] | user: [user name]». So you would need to ask for the user name if you don't know what it is. You can set "unknown" for that response but also ask for name so that you can set the name later.
- Under the heading you must write a summary of everything important in the last request and response.
- You can when ever you want search through previous logs
