"""Assistant-authored dialogue material; no remote model generates scripts.

These are controlled scenario-building blocks, not claimed natural recordings.
Missing languages must fail closed instead of silently using English.
"""

PACKS = {
"en": {
"names": ["Asha", "Daniel", "Maya", "Jonas", "Leila", "Victor", "Nina", "Omar"],
"month": "June",
"opening": "Before we finish, we need a clear decision on {item}. Please separate what is confirmed from what is still a proposal.",
"status": "The previous target for {item} was {old_date}. That depended on getting the missing information early, and we did not get it.",
"question": "What changed since the last update? I thought the main work was finished. Is the remaining problem the cost, or the timing?",
"evidence": "The initial work is finished, but the check found a mismatch. We have {units} affected cases. It is not the whole project, but it is not something we should ignore.",
"proposal": "My first suggestion is to keep the old date and handle the remaining checks afterward. I know that is not ideal, but there is pressure to finish.",
"challenge": "I disagree. If we call this finished before the checks, people will rely on a promise we cannot support. A smaller scope would be more honest.",
"alternative": "We can reduce the scope for {item} and keep the checks. The rest can follow later. That costs less than asking everybody to work another weekend.",
"old_cost": "The draft still says {old_amount}. Is that the total we are being asked to approve? I need to understand what is included before I agree.",
"correct_cost": "No, please correct that. {old_amount} was the earlier option. The revised ceiling is {amount}, including the additional checks. We have not approved spending above that.",
"shortcut": "Could we at least drop the final review? Otherwise I am worried the date will move again, and I will have to explain that to the same people.",
"reject": "No. Skipping the review is rejected. We can change the scope or the date, but the review remains a condition for {item}.",
"commit": "I will prepare the revised plan for {item} by {date}. That includes the scope, the corrected amount and the evidence for the reviewer.",
"owner_question": "Just to be certain, is {other} taking responsibility for that plan? I may have written the wrong name in my notes.",
"owner_correct": "It is {owner}, not {other}. {other} is helping with the review, but the plan and its deadline belong to {owner}.",
"condition": "Then we agree on the reduced scope for {item}, with a ceiling of {amount}. It only goes ahead after the review; this is not unconditional approval.",
"unknown": "One question is still open: whether we need additional support for {item}. We do not have enough information to decide that today.",
"clarify": "Could you repeat the date, please? The audio cut out just as you said it, and I do not want to pass on the old date by mistake.",
"repeat_date": "{date}. I am repeating it because the earlier target, {old_date}, is no longer the commitment.",
"example": "Last time we rushed this kind of change, the handover took longer than the work itself. I would rather make the responsibilities clear now than reconstruct them afterward.",
"constraint": "We also need to account for people who are away. A plan that works only if every person is available all day is not a realistic plan.",
"check_understanding": "I can support that, provided the written version says the same thing. Please do not turn a conditional decision into a firm promise in the update.",
"ack": "Understood. I have corrected my notes. I was looking at the earlier version, which explains why the amount and date did not match.",
"close": "Let us move on. Keep the open question visible, and use the corrected plan for the next update. We do not need to reopen the rejected shortcut.",
},
"hi": {
"names": ["आशा", "विक्रम", "मीरा", "रोहन", "सना", "अर्जुन", "नेहा", "कबीर"],
"month": "जून",
"opening": "आगे बढ़ने से पहले {item} पर साफ़ फैसला चाहिए। जो तय हो चुका है और जो अभी सुझाव है, उन्हें अलग रखें।",
"status": "{item} की पिछली तारीख {old_date} थी। वह ज़रूरी जानकारी समय पर मिलने पर निर्भर थी, और वह जानकारी नहीं मिली।",
"question": "पिछली बातचीत के बाद क्या बदला? मुझे लगा था मुख्य काम पूरा हो गया है। अभी दिक्कत खर्च की है या समय की?",
"evidence": "शुरू का काम पूरा है, लेकिन जाँच में फर्क मिला। {units} मामले प्रभावित हैं। पूरा काम खराब नहीं है, पर इसे नज़रअंदाज़ नहीं कर सकते।",
"proposal": "मेरा पहला सुझाव है कि पुरानी तारीख रखें और बाकी जाँच बाद में करें। यह अच्छा तरीका नहीं है, लेकिन जल्दी पूरा करने का दबाव है।",
"challenge": "मैं सहमत नहीं हूँ। जाँच से पहले पूरा होने का दावा करेंगे तो लोग ऐसे वादे पर भरोसा करेंगे जिसे हम निभा नहीं सकते। दायरा छोटा करना बेहतर है।",
"alternative": "{item} का दायरा कम कर सकते हैं और जाँच रख सकते हैं। बाकी बाद में होगा। सबको फिर सप्ताहांत में काम कराने से यह बेहतर है।",
"old_cost": "मसौदे में अभी भी {old_amount} लिखा है। क्या हमें यही कुल रकम मंज़ूर करनी है? पहले समझना होगा कि इसमें क्या शामिल है।",
"correct_cost": "नहीं, इसे सुधारिए। {old_amount} पहले वाले विकल्प की रकम थी। नई सीमा {amount} है, अतिरिक्त जाँच सहित। उससे ज़्यादा खर्च मंज़ूर नहीं हुआ है।",
"shortcut": "क्या आखिरी समीक्षा छोड़ सकते हैं? नहीं तो तारीख फिर आगे जाएगी और मुझे उन्हीं लोगों को फिर समझाना पड़ेगा।",
"reject": "नहीं। समीक्षा छोड़ने का सुझाव स्वीकार नहीं है। दायरा या तारीख बदल सकती है, लेकिन {item} के लिए समीक्षा ज़रूरी रहेगी।",
"commit": "मैं {date} तक {item} की बदली हुई योजना तैयार करूँगा। उसमें दायरा, सही रकम और समीक्षा के लिए सबूत शामिल होंगे।",
"owner_question": "एक बार पक्का कर लें, क्या यह योजना {other} की ज़िम्मेदारी है? शायद मैंने नोट्स में गलत नाम लिख लिया।",
"owner_correct": "ज़िम्मेदारी {owner} की है, {other} की नहीं। {other} समीक्षा में मदद करेंगे, लेकिन योजना और उसकी समय सीमा {owner} की है।",
"condition": "तो {item} के छोटे दायरे और {amount} की सीमा पर सहमति है। काम समीक्षा के बाद ही आगे जाएगा; यह बिना शर्त मंज़ूरी नहीं है।",
"unknown": "एक सवाल अभी खुला है: क्या {item} के लिए अतिरिक्त मदद चाहिए? आज फैसला करने के लिए पर्याप्त जानकारी नहीं है।",
"clarify": "तारीख फिर बताइए। उसी समय आवाज़ कट गई थी। गलती से पुरानी तारीख आगे नहीं बताना चाहता।",
"repeat_date": "{date}। फिर बता रहा हूँ क्योंकि पहले वाली तारीख {old_date} अब हमारा वादा नहीं है।",
"example": "पिछली बार जल्दबाज़ी में ऐसा बदलाव किया था तो काम से ज़्यादा समय जिम्मेदारियाँ समझाने में लगा। बाद में उलझने से बेहतर है अभी साफ़ कर लें।",
"constraint": "जो लोग छुट्टी पर हैं, उनका भी ध्यान रखना होगा। ऐसी योजना सही नहीं है जो सबके पूरे दिन मौजूद रहने पर ही चल सके।",
"check_understanding": "मैं इससे सहमत हूँ, बशर्ते लिखित जानकारी में भी यही बात हो। शर्त वाले फैसले को पक्का वादा बनाकर मत भेजिए।",
"ack": "समझ गया। मैंने नोट्स सुधार लिए हैं। मैं पुराना मसौदा देख रहा था, इसलिए रकम और तारीख अलग थीं।",
"close": "अब आगे बढ़ते हैं। खुला सवाल दिखाई देता रहे और अगली जानकारी में सुधरी हुई योजना जाए। ठुकराए गए रास्ते पर फिर चर्चा नहीं चाहिए।",
},
"de": {
"names": ["Anna", "Jonas", "Mira", "Lukas", "Lea", "David", "Nina", "Omar"],
"month": "Juni",
"opening": "Bevor wir weitermachen, brauchen wir eine klare Entscheidung zu {item}. Bitte trennt bestätigte Punkte von bloßen Vorschlägen.",
"status": "Der bisherige Termin für {item} war der {old_date}. Dafür hätten die fehlenden Informationen rechtzeitig kommen müssen. Das ist nicht passiert.",
"question": "Was hat sich seit dem letzten Stand geändert? Ich dachte, die Hauptarbeit sei fertig. Geht es jetzt um die Kosten oder um den Termin?",
"evidence": "Die erste Arbeit ist fertig, aber bei der Prüfung gab es eine Abweichung. Betroffen sind {units} Fälle. Nicht das ganze Projekt, aber ignorieren können wir das nicht.",
"proposal": "Mein erster Vorschlag wäre, beim alten Termin zu bleiben und die restlichen Prüfungen danach zu machen. Ideal ist das nicht, aber der Zeitdruck ist da.",
"challenge": "Da widerspreche ich. Wenn wir es vor der Prüfung als fertig bezeichnen, verlassen sich andere auf ein Versprechen, das wir nicht halten können. Lieber ein kleinerer Umfang.",
"alternative": "Wir könnten den Umfang von {item} reduzieren und die Prüfungen beibehalten. Der Rest kommt später. Das ist günstiger als noch ein Wochenende für alle.",
"old_cost": "Im Entwurf steht noch {old_amount}. Ist das die Gesamtsumme, die wir freigeben sollen? Ich möchte zuerst wissen, was darin enthalten ist.",
"correct_cost": "Nein, bitte korrigieren. {old_amount} gehörte zur früheren Variante. Die neue Obergrenze ist {amount}, einschließlich der zusätzlichen Prüfungen. Mehr ist nicht genehmigt.",
"shortcut": "Könnten wir wenigstens die letzte Prüfung weglassen? Sonst verschiebt sich der Termin wieder, und ich muss das denselben Leuten noch einmal erklären.",
"reject": "Nein. Die Prüfung auszulassen, wird abgelehnt. Umfang oder Termin können wir ändern, aber die Prüfung bleibt eine Voraussetzung für {item}.",
"commit": "Ich erstelle bis zum {date} den überarbeiteten Plan für {item}. Darin stehen der Umfang, die korrigierte Summe und die Nachweise für die Prüfung.",
"owner_question": "Nur zur Sicherheit: Ist {other} für diesen Plan verantwortlich? Vielleicht habe ich den falschen Namen notiert.",
"owner_correct": "Verantwortlich ist {owner}, nicht {other}. {other} hilft bei der Prüfung, aber der Plan und sein Termin liegen bei {owner}.",
"condition": "Dann einigen wir uns auf den reduzierten Umfang für {item} und die Obergrenze {amount}. Es geht erst nach der Prüfung weiter. Das ist keine bedingungslose Freigabe.",
"unknown": "Eine Frage bleibt offen: Brauchen wir für {item} zusätzliche Unterstützung? Uns fehlen Informationen, um das heute zu entscheiden.",
"clarify": "Kannst du den Termin wiederholen? Genau da war der Ton weg. Ich möchte nicht versehentlich den alten Termin weitergeben.",
"repeat_date": "Der {date}. Ich wiederhole das, weil der frühere Termin, der {old_date}, nicht mehr zugesagt ist.",
"example": "Beim letzten überstürzten Wechsel dauerte die Übergabe länger als die eigentliche Arbeit. Lieber klären wir die Zuständigkeiten jetzt, statt sie hinterher zu rekonstruieren.",
"constraint": "Wir müssen auch Abwesenheiten einplanen. Ein Plan, der nur funktioniert, wenn alle den ganzen Tag verfügbar sind, ist nicht realistisch.",
"check_understanding": "Damit kann ich leben, sofern es schriftlich genauso steht. Aus einer bedingten Entscheidung darf im Update kein festes Versprechen werden.",
"ack": "Verstanden. Ich habe meine Notizen korrigiert. Ich hatte die alte Fassung vor mir, deshalb passten Betrag und Datum nicht zusammen.",
"close": "Dann weiter zum nächsten Punkt. Die offene Frage bleibt sichtbar, und für das nächste Update gilt der korrigierte Plan. Den abgelehnten Vorschlag müssen wir nicht neu aufrollen.",
},
"pt": {
"names": ["Ana", "Tiago", "Marta", "Rafael", "Inês", "Diogo", "Sofia", "Miguel"],
"month": "de junho",
"opening": "Antes de avançarmos, precisamos de uma decisão clara sobre {item}. Vamos separar o que está confirmado do que ainda é uma proposta.",
"status": "A data anterior para {item} era {old_date}. Isso dependia de recebermos a informação em falta a tempo, e não a recebemos.",
"question": "O que mudou desde a última atualização? Pensei que o trabalho principal estivesse concluído. O problema agora é o custo ou o prazo?",
"evidence": "O trabalho inicial está concluído, mas a verificação encontrou uma diferença. Há {units} casos afetados. Não é o projeto inteiro, mas não podemos ignorar isso.",
"proposal": "A minha primeira proposta é manter a data antiga e fazer as verificações restantes depois. Sei que não é o ideal, mas há pressão para acabar.",
"challenge": "Não concordo. Se dissermos que está concluído antes das verificações, as pessoas vão confiar numa promessa que não conseguimos garantir. Prefiro reduzir o âmbito.",
"alternative": "Podemos reduzir o âmbito de {item} e manter as verificações. O resto fica para depois. Sai mais barato do que pedir outro fim de semana de trabalho a toda a gente.",
"old_cost": "O rascunho ainda diz {old_amount}. É esse o total que estamos a aprovar? Preciso de perceber o que está incluído.",
"correct_cost": "Não, por favor corrijam isso. {old_amount} era a opção anterior. O novo limite é {amount}, incluindo as verificações adicionais. Não foi aprovado gastar mais.",
"shortcut": "Podemos pelo menos dispensar a revisão final? Caso contrário, a data volta a mudar e tenho de explicar tudo outra vez às mesmas pessoas.",
"reject": "Não. A proposta de dispensar a revisão foi rejeitada. Podemos mudar o âmbito ou a data, mas a revisão continua a ser uma condição para {item}.",
"commit": "Preparo o plano revisto de {item} até {date}. Vai incluir o âmbito, o valor corrigido e os elementos necessários para a revisão.",
"owner_question": "Só para confirmar: é {other} que fica responsável por esse plano? Posso ter escrito o nome errado nas notas.",
"owner_correct": "A responsabilidade é de {owner}, não de {other}. {other} ajuda na revisão, mas o plano e o prazo são da responsabilidade de {owner}.",
"condition": "Então concordamos com o âmbito reduzido de {item} e o limite de {amount}. Só avança depois da revisão; não é uma aprovação sem condições.",
"unknown": "Fica uma questão em aberto: precisamos de apoio adicional para {item}? Não temos informação suficiente para decidir hoje.",
"clarify": "Podes repetir a data? O som cortou precisamente nessa altura e não quero transmitir a data antiga por engano.",
"repeat_date": "{date}. Repito porque a data anterior, {old_date}, deixou de ser o compromisso.",
"example": "Da última vez que apressámos uma mudança destas, a passagem de trabalho demorou mais do que a execução. Prefiro esclarecer agora quem faz o quê.",
"constraint": "Também temos de contar com as ausências. Um plano que só funciona se toda a gente estiver disponível o dia inteiro não é realista.",
"check_understanding": "Concordo, desde que a versão escrita diga o mesmo. Não transformem uma decisão condicional numa promessa definitiva na atualização.",
"ack": "Percebido. Já corrigi as notas. Estava a olhar para a versão anterior; por isso, o valor e a data não coincidiam.",
"close": "Vamos ao próximo ponto. A questão em aberto fica visível e a próxima atualização usa o plano corrigido. Não precisamos de voltar à proposta rejeitada.",
},
}


from .locales_europe import EUROPEAN
from .locales_asia import ASIAN
from .locales_more_europe import EUROPE
from .locales_more_asia import ASIA
from .locales_remaining import REMAINING

PACKS.update(EUROPEAN)
PACKS.update(ASIAN)
PACKS.update(EUROPE)
PACKS.update(ASIA)
PACKS.update(REMAINING)


def get_pack(language: str) -> dict:
    if language not in PACKS:
        raise ValueError(f"Assistant-authored dialogue pack is not finished for {language}; English fallback is forbidden")
    return PACKS[language]
