"""Two additional assistant-authored, independent listening trials."""
import json
from pathlib import Path

BASE = Path(__file__).parent
PEOPLE = [
    {"id":"maya", "name":"Maya", "voice":"Leda", "style":"Adult woman, clear natural British English, calm meeting chair. Conversational, not a narrator."},
    {"id":"daniel", "name":"Daniel", "voice":"Puck", "style":"Adult man, natural British English, thoughtful engineer. Conversational, not a narrator."},
    {"id":"sofia", "name":"Sofia", "voice":"Kore", "style":"Adult woman, natural English with a light Spanish accent, friendly and precise. Conversational."},
    {"id":"omar", "name":"Omar", "voice":"Charon", "style":"Adult man, natural English with an Egyptian accent, warm low register. Conversational."},
]
NORMAL = [
("maya", "Morning, everyone. Maya here. Let's settle the library workshop for Saturday. We have ten minutes, and I'd like to leave with a room, a budget, and clear owners."),
("daniel", "Hi, it's Daniel. I checked the laptop room yesterday. The computers work, but only twelve machines have the spreadsheet software installed."),
("sofia", "Sofia here. We have eighteen people registered. Six said they can bring their own laptop, so twelve library computers should be enough."),
("omar", "And Omar here. Before we count on that, did those six people actually confirm? The registration form only asked whether they might bring one."),
("sofia", "Good catch. Five confirmed by email. I am still waiting for the sixth person. I can call them after this meeting."),
("maya", "Then we have one potential shortage, not six. Daniel, is there a spare machine we could borrow for the afternoon?"),
("daniel", "Yes. Reception has a spare, but I need permission before moving it. I can ask today and test the software tomorrow."),
("omar", "The original booking says Saturday at two. The children's reading group finishes at two, though, and they need time to clear the room."),
("maya", "Let's start at half past two instead. Is the room free until four thirty? I don't want us packing up halfway through the exercise."),
("omar", "It is. I checked with the librarian this morning. The two thirty to four thirty slot is available; she is holding it until tomorrow."),
("sofia", "That works for the attendees. I'll send a corrected invitation, but I won't send it until Omar confirms the booking."),
("daniel", "Could we use the upstairs meeting room and start at two as originally planned? It would avoid another email."),
("omar", "The lift is being serviced on Saturday. One attendee uses a wheelchair, so the upstairs room isn't an accessible option."),
("maya", "Then we stay on the ground floor. Accessibility comes first. The confirmed decision is Saturday, two thirty to four thirty, in the laptop room."),
("sofia", "Understood. The draft catering estimate is a hundred and eighty pounds. That includes sandwiches. Do we really need a meal for a two-hour workshop?"),
("omar", "Probably not. Tea, coffee, fruit, and biscuits would be ninety pounds. Water is already available, and we can label the allergens."),
("maya", "Let's cap refreshments at ninety pounds. No sandwiches. Omar, please check the dietary requests before placing the order."),
("omar", "I can do that by Thursday at noon. If the special dietary options push it over ninety, I'll come back to you before ordering."),
("daniel", "For the exercise, I was planning to use the real volunteer contact list. It's a good example of messy data."),
("sofia", "Let's not put personal details on shared computers. Could you make a fictional list with the same kinds of errors instead?"),
("daniel", "Yes, absolutely. I'll replace it with fictional names and addresses. I'll test the exercise and the spare laptop by Friday at three."),
("maya", "Thank you. Sofia, once Omar confirms the room, please send the invitation with the new time and the ground-floor entrance instructions."),
("sofia", "I'll send it by Thursday at four. I'll also call the attendee about bringing their laptop today. Is printed material covered by the refreshments budget?"),
("maya", "No, printing is a separate existing allowance. Please don't take it out of the ninety pounds. We'll use large-print copies where requested."),
("omar", "I will confirm the room this afternoon and message Sofia straight away. One thing is still open: whether we can offer childcare. The volunteer hasn't replied."),
("maya", "Keep childcare marked as unconfirmed. Don't include it as a promise in the invitation. We can send an update if someone agrees to cover it."),
("daniel", "Just checking my list: permission for the spare laptop, a fictional dataset, and the full exercise tested by Friday at three. That's all mine."),
("sofia", "And I have the laptop confirmation call today and the corrected invitation by Thursday at four, after the room is confirmed."),
("omar", "I have the room confirmation today and dietary checks by Thursday at noon. Refreshments must stay within ninety pounds."),
("maya", "Great. Saturday two thirty, ground floor, ninety pounds for refreshments. Childcare is still undecided. Thanks, everyone; we'll check any remaining issues on Friday."),
]
HARD_PEOPLE = [
    {"id":"anna", "name":"Anna", "voice":"Leda", "style":"Adult woman logistics manager, fluent German and English, native German accent, firm and slightly impatient, spontaneous conversation."},
    {"id":"lukas", "name":"Lukas", "voice":"Puck", "style":"Adult man warehouse lead, native German, fluent English with German accent, defensive when interrupted, natural conversation."},
    {"id":"priya", "name":"Priya", "voice":"Kore", "style":"Adult woman customer lead, clear Indian English, urgent but professional, natural conversation."},
    {"id":"ben", "name":"Ben", "voice":"Charon", "style":"Adult man finance analyst, Scottish English accent, low register, precise, sometimes frustrated, natural conversation."},
    {"id":"eva", "name":"Eva", "voice":"Aoede", "style":"Adult woman quality specialist, native German, fluent English with German accent, emphatic and technical, natural conversation."},
]
HARD = [
("anna", "Anna hier. Wir müssen die Lieferung nach Bristol klären. Bitte keine Vermutungen als Fakten weitergeben. Priya, what has the customer actually been told?"),
("priya", "Priya speaking. They were promised forty-eight crates on Thursday morning. Their installation team is already booked. They want a yes or no before five today."),
("lukas", "Lukas hier. Vierundachtzig Kisten stehen auf der Liste, nicht achtundvierzig. Ich habe den Auftrag vor mir. Das Problem ist der falsche Lieferschein.", "counts"),
("priya", "No, forty-eight is the customer order. Eighty-four is the stock transfer. Those are different jobs. Please don't send the whole warehouse to Bristol.", "counts"),
("eva", "Eva hier. Genau. Achtundvierzig bestellt, vierundachtzig im internen Transfer. Aber sechs der achtundvierzig Kisten haben noch kein Prüfsiegel."),
("ben", "Ben here. I heard sixty untested crates. If that's right, we cannot ship anything. Can someone repeat the number without talking over each other?"),
("eva", "Six, not sixty. Sechs Kisten sind noch nicht freigegeben. Zweiundvierzig haben die Prüfung bestanden. Ich spreche langsam, weil die Verbindung schlecht ist."),
("anna", "Danke. Forty-eight ordered, forty-two approved, six awaiting inspection. Lukas, can the forty-two leave today, without mixing in the six?"),
("lukas", "Ja, wenn wir die sechs sofort sperren. Aber die Palette ist schon foliert. Neu sortieren dauert ungefähr eine Stunde. Der normale Laster fährt in vierzig Minuten."),
("priya", "Could we send the six now and email the certificates later? The customer says their team can inspect them on arrival.", "inspection"),
("eva", "Nein. Das ist keine Dokumentenfrage. Bei zwei Kisten gibt es einen offenen Temperaturbefund. Ohne Prüfung werden die sechs nicht versandt.", "inspection"),
("anna", "Agreed. Sending unapproved crates is rejected. Lukas, quarantine those six and prepare the forty-two separately. Don't relabel anything as approved."),
("ben", "The draft says express transport costs nine thousand six hundred euros. We can't approve that against this order's margin."),
("lukas", "Das Angebot war für den gesamten Transfer. Für Bristol allein sind es neunhundertsechzig Euro. Neunhundertsechzig, nicht neuntausendsechshundert."),
("ben", "Okay, nine hundred and sixty for this shipment. Is that the final invoice, or just a ceiling? The notes need to distinguish those."),
("anna", "A ceiling. I approve up to nine hundred and sixty euros for the forty-two cleared crates. Any amount above that comes back to me before booking."),
("priya", "Then I'll tell the customer forty-two crates arrive Thursday morning and the remaining six arrive Friday. That at least gives them something firm."),
("eva", "Freitag ist noch nicht bestätigt. Die Prüfung kann morgen fertig werden, aber wenn der Temperaturbefund bleibt, müssen wir Ersatz suchen."),
("anna", "Please don't promise Friday. The forty-two can be planned for Thursday only after the carrier confirms. There is no delivery commitment for the remaining six."),
("ben", "Who is booking the carrier? I wrote down Priya, but she seems to own the customer conversation rather than transport."),
("lukas", "Ich buche den Transport. Priya spricht mit dem Kunden. Ich brauche die schriftliche Freigabe und die Palettennummern bis sechzehn Uhr dreißig."),
("eva", "Ich schicke dir die Liste der zweiundvierzig freigegebenen Kisten bis sechzehn Uhr fünfzehn. Die sechs gesperrten stehen separat auf einer zweiten Liste."),
("anna", "Good. Lukas books by sixteen thirty, once Eva's approved list is in. Priya, send the customer update by seventeen fifteen, and copy me."),
("priya", "Seventeen fifteen is later than the customer's request for five. I can send an interim update at sixteen forty-five, then the final transport details at seventeen fifteen."),
("anna", "Yes. Interim update at sixteen forty-five, confirmed transport details by seventeen fifteen. Explain that the six crates remain under inspection, with no promised arrival date."),
("ben", "I've just received the carrier note. It says nine hundred and sixty excludes a two hundred euro fuel charge. So the current quote is eleven hundred and sixty.", "cost"),
("lukas", "Dann passt das Angebot nicht zur Freigabe. Ich kann einen anderen Anbieter anrufen. Bitte nicht einfach die Grenze erhöhen, bevor wir verglichen haben.", "cost"),
("anna", "Keep the ceiling at nine hundred and sixty. Lukas, ask for an all-inclusive quote. If no carrier accepts, escalate to me; don't book over the limit."),
("eva", "Die sechs Kisten sind jetzt im System gesperrt. Das ist erledigt. Die Liste für die zweiundvierzig fehlt noch; die schicke ich wie vereinbart um sechzehn Uhr fünfzehn."),
("ben", "So quarantining the six is completed, but the approved list and transport booking are still open. I'll record the extra fuel charge as an unapproved proposal."),
("priya", "And the customer's installation cancellation fee? They mentioned it in their message, but I haven't accepted responsibility or agreed to pay it."),
("anna", "Leave that unresolved. Send the claim to legal for review, Priya, by tomorrow at ten. This call does not approve compensation."),
("eva", "Zur Zusammenfassung: achtundvierzig bestellt, zweiundvierzig freigegeben, sechs gesperrt. Für die sechs gibt es noch keinen Liefertermin. Keine Freigabe ohne Prüfung."),
("lukas", "Ich hole ein Angebot inklusive aller Zuschläge und buche bis sechzehn Uhr dreißig, wenn es unter neunhundertsechzig Euro bleibt. Sonst melde ich mich bei Anna."),
("priya", "I send an interim customer update at sixteen forty-five, transport details by seventeen fifteen, and the compensation claim to legal by ten tomorrow. No promise for the six."),
("anna", "Exactly. The Thursday plan for forty-two crates depends on carrier confirmation. Quarantine is done, inspection and booking are still open. We'll regroup at sixteen thirty-five."),
]

def save(filename, title, people, rows, languages, difficulty, profile, facts):
    turns=[dict(id=f"T{i:03d}",speaker=row[0],text=row[1],emotion='angry' if len(row)>2 else 'calm', **({'group':row[2]} if len(row)>2 else {})) for i,row in enumerate(rows,1)]
    data=dict(id=filename[:-5],title=title,count_toward_200=False,reference_status="Assistant-authored synthetic listening trial; not native-speaker validated",people=people,turns=turns,languages=languages,difficulty=difficulty,audio_profile=profile,facts=facts)
    (BASE/filename).write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')

if __name__=='__main__':
    save('normal_example.json','[Synthetic normal] Library workshop — four speakers',PEOPLE,NORMAL,['en'],'normal',
         dict(interruption_intensity=0,snr_db=32,remote_speakers=[],echo_speakers=[],packet_loss=0),[
         dict(id='D1',text='Saturday 14:30–16:30, accessible ground-floor laptop room; upstairs rejected.',sources=['T014','T030']),
         dict(id='D2',text='Refreshments capped at GBP 90; printing separate; no sandwiches.',sources=['T017','T024']),
         dict(id='A1',text='Daniel: spare laptop permission and fictional dataset/exercise tested by Friday 15:00.',sources=['T021','T027']),
         dict(id='A2',text='Sofia: laptop confirmation today, invitation Thursday 16:00 after booking.',sources=['T023','T028']),
         dict(id='A3',text='Omar: room confirmation today, dietary checks Thursday noon.',sources=['T025','T029']),
         dict(id='O1',text='Childcare is unconfirmed; invitation must not promise it.',sources=['T025','T026'])])
    save('hard_example.json','[Synthetic hard] Bristol shipment — English + German, five speakers',HARD_PEOPLE,HARD,['en','de'],'hard',
         dict(interruption_intensity=.6,snr_db=19,remote_speakers=['eva'],echo_speakers=[],packet_loss=.025),[
         dict(id='F1',text='48 ordered, 84 internal transfer, 42 approved, 6 unapproved; six not sixty.',sources=['T003','T004','T007','T033']),
         dict(id='D1',text='Unapproved six may not ship; quarantine completed; their delivery date remains unknown.',sources=['T012','T029','T033']),
         dict(id='D2',text='EUR 960 all-inclusive spending ceiling unchanged; EUR 1160 including fuel unapproved.',sources=['T026','T028']),
         dict(id='A1',text='Eva sends approved list by 16:15; Lukas books by 16:30 only within ceiling, otherwise escalates to Anna.',sources=['T022','T023','T034']),
         dict(id='A2',text='Priya interim 16:45, transport details 17:15; legal compensation referral by 10:00 tomorrow.',sources=['T025','T032','T035']),
         dict(id='O1',text='Thursday for 42 depends on carrier; Friday for six rejected; compensation not approved.',sources=['T019','T032','T036'])])
