"""Assistant-authored controlled dialogue; independent native review pending."""
KEYS = 'opening status question evidence proposal challenge alternative old_cost correct_cost shortcut reject commit owner_question owner_correct condition unknown clarify repeat_date example constraint check_understanding ack close'.split()

def pack(names, month, lines):
    rows=lines.strip().splitlines()
    assert len(rows)==len(KEYS), (len(rows),len(KEYS))
    return {'names':names.split('|'),'month':month,**dict(zip(KEYS,rows))}

EUROPE = {
'ru':pack('Анна|Дмитрий|Мария|Иван|Елена|Алексей|Ольга|Павел','июня','''
Прежде чем идти дальше, надо решить вопрос о {item}. Отделим подтверждённые решения от предложений.
Для {item} раньше называли {old_date}. Этот срок зависел от данных, которые мы вовремя не получили.
Что изменилось после прошлого обсуждения? Нас задерживает стоимость или сам график?
Основная работа закончена, но проверка выявила расхождение. Оно затрагивает {units} случаев, а не весь проект.
Предлагаю пока сохранить прежнюю дату, а оставшиеся проверки закончить после запуска.
Я против. Нельзя обещать готовность, пока проверка не закончена. Давайте лучше сократим объём.
Можно уменьшить объём работ по {item}, но сохранить проверку. Остальное сделаем следующим этапом.
В черновике стоит сумма {old_amount}. Это полная стоимость или только часть расходов?
Это старая сумма. Новый предел составляет {amount}, включая проверку. Превышение никто не согласовывал.
А последнюю проверку можно пропустить? Иначе нам опять придётся объяснять перенос срока.
Нет, пропускать проверку нельзя. Для {item} она остаётся обязательным условием.
Я подготовлю исправленный план по {item} к {date}. Укажу объём, стоимость и материалы для проверки.
Правильно ли я записал, что за план отвечает {other}? Возможно, я перепутал имена.
За план отвечает {owner}, а не {other}. {other} помогает с проверкой, но не отвечает за срок.
Согласовали сокращённый объём по {item} и предел {amount}. Продолжение возможно только после проверки.
Остаётся открытый вопрос: понадобится ли дополнительная помощь для {item}? Пока данных недостаточно.
Повторите, пожалуйста, дату. Именно в этот момент пропал звук, и я мог записать старый срок.
Новая дата — {date}. Ранее названная дата {old_date} больше не является обещанием.
В прошлый раз мы поторопились и потом долго выясняли, кто за что отвечает. Не хочу повторения.
Нужно учитывать отпуска. План не должен зависеть от того, что все постоянно доступны.
Согласен, если в письме будет та же формулировка. Условное решение нельзя выдавать за окончательное.
Понятно, я исправил запись. У меня был открыт старый черновик с другой суммой и датой.
Переходим дальше. Открытый вопрос оставляем в списке, а в следующем сообщении используем исправленный план.
'''),
'pl':pack('Anna|Piotr|Marta|Jan|Ewa|Tomasz|Zofia|Adam','czerwca','''
Zanim przejdziemy dalej, ustalmy decyzję dotyczącą {item}. Oddzielmy ustalenia od propozycji.
Poprzedni termin dla {item} to {old_date}. Zakładał wcześniejsze otrzymanie danych, których nie dostaliśmy.
Co zmieniło się od ostatniej rozmowy? Problem dotyczy kosztu czy harmonogramu?
Główna praca jest gotowa, ale kontrola wykazała rozbieżność w {units} przypadkach. Nie chodzi o cały projekt.
Proponuję zachować stary termin, a pozostałe kontrole wykonać później. Wiem, że to nieidealne.
Nie zgadzam się. Bez kontroli nie możemy obiecywać gotowości. Lepiej ograniczyć zakres.
Możemy zmniejszyć zakres {item} i zachować kontrolę. Resztę zrobimy w kolejnym etapie.
W projekcie nadal widnieje {old_amount}. Czy to pełna kwota, czy tylko część wydatków?
To stara kwota. Nowy limit wynosi {amount}, razem z kontrolą. Nie zatwierdziliśmy przekroczenia.
Czy możemy chociaż pominąć końcowy przegląd? Inaczej znowu trzeba będzie tłumaczyć zmianę terminu.
Nie. Pominięcie przeglądu odrzucamy. Dla {item} pozostaje on warunkiem dalszych prac.
Przygotuję poprawiony plan dla {item} do {date}, z zakresem, kosztem i materiałami do przeglądu.
Czy za plan odpowiada {other}? Być może zanotowałem niewłaściwe imię.
Odpowiada {owner}, nie {other}. {other} pomaga przy przeglądzie, ale nie odpowiada za plan i termin.
Uzgadniamy mniejszy zakres {item} i limit {amount}. Dalsze działania zależą od wyniku przeglądu.
Otwarta pozostaje kwestia dodatkowego wsparcia dla {item}. Dziś nie mamy dość informacji, żeby zdecydować.
Powtórz proszę termin. Właśnie wtedy zniknął dźwięk i nie chcę przekazać starej daty.
Nowy termin to {date}. Poprzednia data, {old_date}, nie jest już naszym zobowiązaniem.
Ostatnio pospieszyliśmy się i przekazanie pracy trwało dłużej niż wykonanie. Ustalmy teraz odpowiedzialność.
Musimy uwzględnić urlopy. Plan nie może wymagać stałej obecności wszystkich osób.
Zgoda, jeśli pisemna informacja będzie mówiła to samo. Warunkowa decyzja nie może stać się pewną obietnicą.
Rozumiem. Poprawiłem notatki; miałem otwartą poprzednią wersję z inną kwotą i datą.
Przejdźmy dalej. Zostawmy otwartą kwestię na liście i użyjmy poprawionego planu w następnej informacji.
'''),
'cs':pack('Anna|Petr|Marie|Jan|Eva|Tomáš|Lucie|Adam','června','''
Než půjdeme dál, rozhodněme o {item}. Oddělme potvrzené závěry od pouhých návrhů.
Původní termín pro {item} byl {old_date}. Počítal s podklady, které jsme včas nedostali.
Co se změnilo od minulé porady? Zdržuje nás cena, nebo harmonogram?
Hlavní práce je hotová, ale kontrola našla nesrovnalost v {units} případech. Nejde o celý projekt.
Navrhuji zachovat původní termín a zbývající kontroly dokončit potom. Vím, že to není ideální.
Nesouhlasím. Bez kontroly nemůžeme slíbit dokončení. Raději omezme rozsah.
Můžeme zmenšit rozsah {item} a zachovat kontrolu. Zbytek přesuneme do další etapy.
V návrhu je stále {old_amount}. Je to celková částka, nebo jen část nákladů?
To je stará částka. Nový limit je {amount}, včetně kontroly. Vyšší výdaje schválené nejsou.
Nemohli bychom vynechat závěrečnou kontrolu? Jinak budu znovu vysvětlovat posun termínu.
Ne. Vynechání kontroly zamítáme. Pro {item} zůstává podmínkou dalšího postupu.
Upravený plán pro {item} připravím do {date}. Bude obsahovat rozsah, cenu a podklady ke kontrole.
Zapsal jsem si, že za plán odpovídá {other}. Je to správně?
Odpovídá {owner}, nikoli {other}. {other} pomáhá s kontrolou, ale plán i termín má na starosti {owner}.
Dohodli jsme menší rozsah {item} a limit {amount}. Pokračování závisí na kontrole.
Otevřená zůstává otázka, zda {item} potřebuje další podporu. Dnes nemáme dost informací.
Zopakuj prosím datum. Právě tehdy vypadl zvuk a nechci předat starý termín.
Nový termín je {date}. Původní datum {old_date} už neplatí jako příslib.
Minule jsme spěchali a předání trvalo déle než samotná práce. Vyjasněme si odpovědnost hned.
Musíme počítat s dovolenými. Plán nemůže stát na tom, že jsou všichni pořád dostupní.
Souhlasím, pokud písemné shrnutí řekne totéž. Podmíněné rozhodnutí není jistý příslib.
Rozumím, opravil jsem si poznámky. Díval jsem se na starý návrh s jinou cenou a datem.
Pojďme dál. Otevřenou otázku necháme v seznamu a příště použijeme upravený plán.
'''),
'hu':pack('Anna|Péter|Éva|Gábor|Júlia|Dániel|Nóra|Ádám','június','''
Mielőtt továbblépünk, döntsünk erről: {item}. Válasszuk külön a biztos döntést és a javaslatot.
A korábbi határidő {old_date} volt. Ehhez időben meg kellett volna kapnunk az adatokat, de nem érkeztek meg.
Mi változott a múltkori egyeztetés óta? A költség vagy az időbeosztás okozza a gondot?
Az alapmunka kész, de az ellenőrzés {units} esetben eltérést talált. Nem az egész projektről van szó.
Azt javaslom, tartsuk a régi dátumot, és a maradék ellenőrzést később fejezzük be.
Ezzel nem értek egyet. Ellenőrzés nélkül nem ígérhetünk kész munkát. Inkább csökkentsük a terjedelmet.
A {item} esetében csökkenthetjük a feladat terjedelmét, de az ellenőrzés maradjon meg.
A tervezetben még {old_amount} szerepel. Ez a teljes összeg, vagy csak a költség egy része?
Ez a régi összeg. Az új keret {amount}, az ellenőrzéssel együtt. Többet nem hagytunk jóvá.
Legalább a végső ellenőrzést kihagyhatnánk? Különben megint magyarázhatom a határidő módosítását.
Nem. Az ellenőrzés kihagyását elutasítjuk. A {item} csak ellenőrzés után folytatható.
A módosított tervet {date} időpontig elkészítem, a terjedelemmel, költséggel és az ellenőrzés anyagával.
Jól írtam fel, hogy {other} felel a tervért? Lehet, hogy összekevertem a neveket.
A felelős {owner}, nem {other}. Az ellenőrzésben {other} segít, de a tervért {owner} felel.
Megegyeztünk a {item} kisebb terjedelmében és a {amount} keretben. A folytatás feltétele az ellenőrzés.
Nyitott kérdés, hogy kell-e további segítség a {item} feladathoz. Ma még nincs elég információnk.
Kérlek, ismételd meg a dátumot. Akkor szakadt meg a hang, és nem akarok régi határidőt továbbadni.
Az új határidő {date}. A korábbi {old_date} már nem vállalt időpont.
Legutóbb siettünk, aztán az átadás tovább tartott a munkánál. Most tisztázzuk a felelősségeket.
A szabadságokat is figyelembe kell venni. A terv ne igényelje mindenki állandó jelenlétét.
Rendben, ha az írásos összefoglaló is ezt mondja. A feltételes döntésből ne legyen biztos ígéret.
Értem, javítottam a jegyzeteimet. A régi változat volt előttem, más összeggel és dátummal.
Lépjünk tovább. A nyitott kérdés maradjon a listán, és az új tervet használjuk a következő tájékoztatóban.
'''),
'mk':pack('Ана|Петар|Марија|Иван|Елена|Даниел|Нина|Александар','јуни','''
Пред да продолжиме, да одлучиме за {item}. Да ги одвоиме потврдените одлуки од предлозите.
Претходниот рок за {item} беше {old_date}. Зависеше од податоци што не ги добивме навреме.
Што се смени од минатиот состанок? Проблемот е во трошоците или во распоредот?
Главната работа е завршена, но проверката откри разлика во {units} случаи. Не е проблем целиот проект.
Предлагам да го задржиме стариот датум, а преостанатите проверки да ги завршиме подоцна.
Не се согласувам. Без проверка не можеме да ветиме дека е готово. Подобро да го намалиме обемот.
Можеме да го намалиме обемот за {item}, но проверката да остане. Остатокот ќе биде следна фаза.
Во нацртот сè уште стои {old_amount}. Дали тоа е вкупниот износ или само дел од трошокот?
Тоа е стариот износ. Новата граница е {amount}, со вклучена проверка. Повеќе не е одобрено.
Може ли барем да ја прескокнеме последната проверка? Инаку пак ќе го објаснуваме поместувањето.
Не. Предлогот за прескокнување е одбиен. Проверка е услов за да продолжи {item}.
Ќе го подготвам поправениот план за {item} до {date}, со обемот, трошокот и материјалите за проверка.
Дали за планот е одговорен {other}? Можеби погрешно го запишав името.
Одговорен е {owner}, не {other}. {other} помага со проверката, но планот и рокот се на {owner}.
Договоривме намален обем за {item} и граница од {amount}. Продолжуваме само по проверката.
Останува отворено дали е потребна дополнителна помош за {item}. Денес немаме доволно информации.
Повтори го датумот, те молам. Токму тогаш се прекина звукот и не сакам да го пренесам стариот рок.
Новиот датум е {date}. Претходниот датум {old_date} веќе не е наше ветување.
Минатиот пат брзавме и предавањето траеше подолго од работата. Сега да ги разјасниме одговорностите.
Мора да ги земеме предвид и одморите. Планот не смее да бара сите постојано да бидат достапни.
Се согласувам ако и писмената порака го кажува истото. Условната одлука не е сигурно ветување.
Разбирам, ги поправив белешките. Ја гледав старата верзија со друг износ и датум.
Да продолжиме. Отвореното прашање останува на списокот, а следната порака ќе го користи поправениот план.
'''),
'ro':pack('Ana|Andrei|Maria|Mihai|Elena|Daniel|Ioana|Radu','iunie','''
Înainte să continuăm, trebuie să decidem în privința {item}. Să separăm deciziile confirmate de propuneri.
Termenul anterior pentru {item} era {old_date}. Depindea de informații pe care nu le-am primit la timp.
Ce s-a schimbat de la ultima discuție? Problema ține de cost sau de program?
Lucrarea principală este gata, dar verificarea a găsit o diferență în {units} cazuri. Nu e întregul proiect.
Propun să păstrăm data veche și să terminăm verificările rămase după aceea. Știu că nu este ideal.
Nu sunt de acord. Fără verificare nu putem promite că e gata. Mai bine reducem volumul.
Putem reduce volumul pentru {item}, păstrând verificarea. Restul intră în etapa următoare.
În ciornă apare încă suma {old_amount}. Acesta este costul total sau doar o parte?
Aceea este suma veche. Noul plafon este {amount}, cu verificarea inclusă. Nu am aprobat mai mult.
Putem sări măcar peste verificarea finală? Altfel trebuie să explicăm din nou schimbarea termenului.
Nu. Propunerea de a sări verificarea este respinsă. Ea rămâne o condiție pentru {item}.
Pregătesc planul revizuit pentru {item} până la {date}, cu volumul, costul și materialele pentru verificare.
Am notat că {other} răspunde de plan. Este corect sau am încurcat numele?
Răspunde {owner}, nu {other}. {other} ajută la verificare, dar planul și termenul îi revin lui {owner}.
Am convenit un volum redus pentru {item} și plafonul {amount}. Continuarea depinde de verificare.
Rămâne deschisă întrebarea dacă avem nevoie de ajutor suplimentar pentru {item}. Azi nu avem suficiente date.
Poți repeta data? Atunci s-a întrerupt sunetul și nu vreau să transmit termenul vechi.
Noul termen este {date}. Data anterioară, {old_date}, nu mai este un angajament.
Data trecută ne-am grăbit, iar predarea a durat mai mult decât lucrarea. Să clarificăm acum responsabilitățile.
Trebuie să ținem cont de concedii. Planul nu poate cere ca toată lumea să fie disponibilă permanent.
Sunt de acord dacă mesajul scris spune același lucru. O decizie condiționată nu este o promisiune sigură.
Am înțeles și am corectat notițele. Mă uitam la versiunea veche, cu altă sumă și altă dată.
Să continuăm. Lăsăm întrebarea deschisă pe listă, iar următorul mesaj folosește planul corectat.
'''),
'sv':pack('Anna|Erik|Maria|Johan|Elin|David|Nora|Oskar','juni','''
Innan vi går vidare behöver vi besluta om {item}. Skilj på bekräftade beslut och förslag.
Det tidigare datumet för {item} var {old_date}. Det byggde på underlag som inte kom i tid.
Vad har ändrats sedan sist? Är det kostnaden eller tidsplanen som ställer till det?
Grundarbetet är klart, men kontrollen visade en avvikelse i {units} fall. Det gäller inte hela projektet.
Jag föreslår att vi behåller det gamla datumet och gör de sista kontrollerna efteråt.
Det håller jag inte med om. Vi kan inte lova att det är klart utan kontroll. Minska hellre omfattningen.
Vi kan minska omfattningen för {item} men behålla kontrollen. Resten tar vi i nästa etapp.
Utkastet säger fortfarande {old_amount}. Är det hela kostnaden eller bara en del?
Det är den gamla summan. Det nya taket är {amount}, inklusive kontroll. Mer är inte godkänt.
Kan vi åtminstone hoppa över slutkontrollen? Annars får vi förklara en försening igen.
Nej. Förslaget att hoppa över kontrollen avvisas. Den är fortfarande ett villkor för {item}.
Jag tar fram en reviderad plan för {item} till {date}, med omfattning, kostnad och kontrollunderlag.
Jag skrev att {other} ansvarar för planen. Stämmer det, eller blandade jag ihop namnen?
Det är {owner}, inte {other}. {other} hjälper till med kontrollen, men {owner} ansvarar för planen och tiden.
Vi enas om mindre omfattning för {item} och taket {amount}. Fortsättningen kräver godkänd kontroll.
Det är fortfarande oklart om {item} behöver mer hjälp. Vi har för lite information för att avgöra det idag.
Kan du upprepa datumet? Ljudet försvann just då, och jag vill inte sprida den gamla tiden.
Det nya datumet är {date}. Det tidigare datumet, {old_date}, gäller inte längre som löfte.
Förra gången skyndade vi oss och överlämningen tog längre tid än arbetet. Klargör ansvaret nu.
Vi måste också räkna med semestrar. Planen kan inte kräva att alla alltid är tillgängliga.
Jag håller med om det står likadant i det skriftliga beskedet. Gör inte ett villkorat beslut till ett säkert löfte.
Jag förstår, och har rättat anteckningarna. Jag tittade på ett gammalt utkast med annan summa och annat datum.
Då går vi vidare. Den öppna frågan ligger kvar, och nästa uppdatering ska använda den rättade planen.
'''),
'da':pack('Anna|Mikkel|Maria|Lars|Sofie|Jonas|Ida|Emil','juni','''
Inden vi går videre, skal vi beslutte noget om {item}. Lad os skille aftaler fra forslag.
Den tidligere dato for {item} var {old_date}. Den krævede oplysninger, som vi ikke fik i tide.
Hvad er ændret siden sidst? Er det prisen eller tidsplanen, der giver problemer?
Grundarbejdet er færdigt, men kontrollen fandt en forskel i {units} tilfælde. Det gælder ikke hele projektet.
Jeg foreslår at holde den gamle dato og gøre de sidste kontroller bagefter.
Det er jeg uenig i. Vi kan ikke love færdigt arbejde uden kontrol. Reducér hellere omfanget.
Vi kan reducere omfanget af {item} og beholde kontrollen. Resten tager vi i næste fase.
Udkastet siger stadig {old_amount}. Er det hele beløbet eller kun en del af udgiften?
Det er det gamle beløb. Det nye loft er {amount}, inklusive kontrollen. Mere er ikke godkendt.
Kan vi i det mindste springe slutkontrollen over? Ellers skal vi forklare endnu en udsættelse.
Nej. Forslaget om at springe kontrollen over afvises. Den er stadig en betingelse for {item}.
Jeg laver en rettet plan for {item} inden {date}, med omfang, pris og materiale til kontrollen.
Jeg skrev, at {other} har ansvaret for planen. Er det rigtigt, eller har jeg byttet om på navnene?
Det er {owner}, ikke {other}. {other} hjælper med kontrollen, men {owner} har ansvar for planen og fristen.
Vi aftaler et mindre omfang for {item} og et loft på {amount}. Vi fortsætter først efter kontrollen.
Det er stadig åbent, om {item} kræver ekstra hjælp. Vi har ikke nok oplysninger til at afgøre det i dag.
Kan du gentage datoen? Lyden forsvandt netop dér, og jeg vil ikke sende den gamle dato videre.
Den nye dato er {date}. Den tidligere dato, {old_date}, er ikke længere vores løfte.
Sidst skyndte vi os, og overdragelsen tog længere tid end arbejdet. Lad os afklare ansvaret nu.
Vi skal også tage højde for ferie. Planen kan ikke kræve, at alle altid er til stede.
Enig, hvis den skriftlige besked siger det samme. En betinget beslutning må ikke blive et sikkert løfte.
Forstået. Jeg har rettet noterne; jeg så på det gamle udkast med en anden pris og dato.
Lad os gå videre. Det åbne spørgsmål bliver på listen, og næste besked bruger den rettede plan.
'''),
'fi':pack('Anna|Mikko|Maria|Juho|Elina|Antti|Nora|Oskari','kesäkuuta','''
Ennen kuin jatkamme, päätetään asiasta {item}. Erotetaan vahvistetut päätökset ehdotuksista.
Aiempi määräpäivä oli {old_date}. Se edellytti tietoja, joita emme saaneet ajoissa.
Mikä muuttui viime keskustelusta? Onko ongelma kustannuksissa vai aikataulussa?
Perustyö on valmis, mutta tarkastuksessa löytyi poikkeama {units} tapauksesta. Kyse ei ole koko hankkeesta.
Ehdotan vanhan päivämäärän säilyttämistä ja viimeisten tarkastusten tekemistä sen jälkeen.
Olen eri mieltä. Ilman tarkastusta emme voi luvata valmista. Rajataan mieluummin työn laajuutta.
Voimme rajata kokonaisuutta {item} ja säilyttää tarkastuksen. Loput tehdään seuraavassa vaiheessa.
Luonnoksessa on yhä {old_amount}. Onko se koko summa vai vain osa kustannuksista?
Se on vanha summa. Uusi enimmäismäärä on {amount}, tarkastus mukaan lukien. Enempää ei ole hyväksytty.
Voisimmeko jättää edes lopputarkastuksen pois? Muuten joudumme taas selittämään aikataulun muutosta.
Emme. Tarkastuksen ohittaminen hylätään. Se on edelleen edellytys kokonaisuuden {item} jatkamiselle.
Laadin korjatun suunnitelman aiheesta {item} määräpäivään {date} mennessä. Siinä ovat laajuus, hinta ja tarkastusaineisto.
Kirjasinko oikein, että suunnitelman vastuuhenkilö on {other}? Saatoin sekoittaa nimet.
Vastuuhenkilö on {owner}, ei {other}. Tarkastuksessa auttaa {other}, mutta suunnitelmasta ja ajasta vastaa {owner}.
Sovimme rajatusta kokonaisuudesta {item} ja enimmäismäärästä {amount}. Jatko edellyttää tarkastusta.
Auki jää, tarvitaanko kokonaisuuteen {item} lisää apua. Tiedot eivät riitä päätökseen tänään.
Voitko toistaa päivämäärän? Ääni katkesi juuri silloin, enkä halua välittää vanhaa aikaa.
Uusi päivämäärä on {date}. Aiempi {old_date} ei enää ole lupauksemme.
Viimeksi kiirehdimme, ja luovutus kesti työtä kauemmin. Selvennetään vastuut tällä kertaa etukäteen.
Myös lomat pitää huomioida. Suunnitelma ei voi edellyttää kaikkien jatkuvaa läsnäoloa.
Sopii, jos kirjallinen viesti sanoo saman. Ehdollista päätöstä ei saa muuttaa varmaksi lupaukseksi.
Selvä, korjasin muistiinpanot. Katsoin vanhaa luonnosta, jossa summa ja päivämäärä olivat toiset.
Jatketaan. Avoin kysymys jää listalle, ja seuraavassa viestissä käytetään korjattua suunnitelmaa.
'''),
}
