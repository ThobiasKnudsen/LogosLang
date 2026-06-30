# Follow Elon Musks rules faithfully:
1. **Question every requirement:** Find out who exactly created the requirement and never just accept that it's from "the safety or legal department". Make the requirements less dumb.
2. **Delete parts or processes:** Delete as many parts or steps as you can. Musk's rule of thumb: If you aren't forced to put back at least 10% of what you deleted, you didn't delete enough.
3. **Simplify and optimize:** Only do this after you have completed steps 1 and 2, because a massive waste of time is optimizing something that shouldn't exist in the first place.
4. **Accelerate cycle time:** Speed up the process.
5. **Automate:** Only automate the process after the first four steps are done.

# Follow logging rules faithfully:
- At session start create a file under CLAUDE_LOG folder with the name Session_YYYY-MM-DD_HH:mm:ss.md. 
- What should be said in the start of the file is the session id so that Claude can find the actual session later for more details. Format like this «# Session id: [session id]»
- At the end of each response for the user request you always append to the file, starting with a heading on this format «## response time: YYYY-MM-DD_HH:mm:ss | LLM: [LLM model responding] | user: [user name]». So you would need to ask for the user name if you don't know what it is. You can set "unknown" for that response but also ask for name so that you can set the name later.
- Under the heading you must write a summary of everything important in the last request and response.
- You can when ever you want search through previous logs
