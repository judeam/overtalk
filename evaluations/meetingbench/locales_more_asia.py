"""Assistant-authored controlled dialogue; native-language review still pending."""
from .locales_more_europe import pack
ASIA = {
'id':pack('Ayu|Budi|Sari|Dimas|Rina|Arif|Nadia|Fajar','Juni','''
Sebelum lanjut, kita perlu keputusan tentang {item}. Tolong bedakan keputusan yang sudah pasti dari usulan.
Jadwal sebelumnya untuk {item} adalah {old_date}. Itu bergantung pada data yang ternyata terlambat kita terima.
Apa yang berubah sejak pembaruan terakhir? Masalahnya biaya atau jadwal?
Pekerjaan utama selesai, tetapi pemeriksaan menemukan selisih pada {units} kasus. Bukan seluruh proyek yang bermasalah.
Usulan saya, tanggal lama dipertahankan dan pemeriksaan sisanya dilakukan sesudahnya. Saya tahu ini kurang ideal.
Saya tidak setuju. Kita belum bisa menjanjikan selesai tanpa pemeriksaan. Lebih baik lingkupnya diperkecil.
Lingkup {item} bisa dikurangi, tetapi pemeriksaannya tetap. Sisanya kita kerjakan pada tahap berikutnya.
Draf masih mencantumkan {old_amount}. Itu seluruh biaya atau baru sebagian?
Itu angka lama. Batas yang baru adalah {amount}, termasuk pemeriksaan. Belum ada persetujuan untuk lebih dari itu.
Bagaimana kalau pemeriksaan terakhir saja dilewati? Kalau tidak, kita harus menjelaskan perubahan jadwal lagi.
Tidak. Usulan melewati pemeriksaan ditolak. Pemeriksaan tetap menjadi syarat untuk {item}.
Saya siapkan rencana revisi untuk {item} paling lambat {date}, termasuk lingkup, biaya, dan bahan pemeriksaan.
Saya mencatat {other} sebagai penanggung jawab rencana. Benar begitu, atau saya salah nama?
Penanggung jawabnya {owner}, bukan {other}. {other} membantu pemeriksaan, tetapi rencana dan tenggat tanggung jawab {owner}.
Kita menyepakati lingkup yang lebih kecil untuk {item} dengan batas {amount}. Pelaksanaannya tetap menunggu pemeriksaan.
Masih terbuka apakah {item} membutuhkan bantuan tambahan. Informasinya belum cukup untuk diputuskan hari ini.
Bisa ulangi tanggalnya? Suara tadi putus, dan saya tidak ingin meneruskan jadwal lama.
Tanggal barunya {date}. Tanggal sebelumnya, {old_date}, bukan lagi janji kita.
Terakhir kita terburu-buru, serah terimanya justru lebih lama daripada pekerjaannya. Mari perjelas tanggung jawab sekarang.
Kita juga perlu memperhitungkan cuti. Rencana tidak boleh bergantung pada semua orang selalu tersedia.
Saya setuju kalau informasi tertulisnya juga begitu. Keputusan bersyarat jangan diubah menjadi janji pasti.
Baik, catatan saya sudah diperbaiki. Tadi saya membuka draf lama dengan angka dan tanggal berbeda.
Kita lanjut. Pertanyaan yang belum terjawab tetap dicatat, dan pembaruan berikutnya memakai rencana yang sudah diperbaiki.
'''),
'ms':pack('Aina|Amir|Siti|Hakim|Farah|Azlan|Nadia|Rizal','Jun','''
Sebelum kita teruskan, perlu ada keputusan tentang {item}. Bezakan perkara yang disahkan dengan cadangan.
Tarikh asal bagi {item} ialah {old_date}. Ia bergantung pada maklumat yang tidak sampai tepat pada masanya.
Apa yang berubah sejak perbincangan lalu? Masalahnya kos atau jadual?
Kerja utama sudah selesai, tetapi semakan menemui perbezaan dalam {units} kes. Bukan keseluruhan projek terjejas.
Saya cadangkan kita kekalkan tarikh lama dan buat baki semakan selepas itu. Saya tahu ia kurang sesuai.
Saya tak setuju. Tanpa semakan, kita belum boleh janji semuanya siap. Lebih baik kurangkan skop.
Skop {item} boleh dikurangkan, tetapi semakan dikekalkan. Bakinya dibuat pada fasa seterusnya.
Draf masih mencatat {old_amount}. Itu jumlah keseluruhan atau hanya sebahagian kos?
Itu angka lama. Had baharu ialah {amount}, termasuk semakan. Jumlah lebih tinggi belum diluluskan.
Boleh kita langkau semakan akhir sahaja? Jika tidak, kita perlu menjelaskan penangguhan lagi.
Tidak. Cadangan melangkau semakan ditolak. Semakan tetap menjadi syarat bagi {item}.
Saya sediakan pelan yang dipinda bagi {item} sebelum {date}, dengan skop, kos dan bahan semakan.
Saya catat {other} sebagai orang yang bertanggungjawab. Betul, atau saya tersalah nama?
Yang bertanggungjawab ialah {owner}, bukan {other}. {other} membantu semakan, tetapi pelan dan tarikh akhir milik {owner}.
Kita setuju dengan skop lebih kecil bagi {item} dan had {amount}. Kerja seterusnya bergantung pada semakan.
Masih belum diputuskan sama ada {item} memerlukan bantuan tambahan. Maklumat hari ini belum mencukupi.
Boleh ulang tarikh itu? Bunyi terputus tadi dan saya tak mahu sampaikan tarikh lama.
Tarikh baharu ialah {date}. Tarikh lama, {old_date}, bukan lagi janji kita.
Dulu kita tergesa-gesa dan penyerahan kerja mengambil masa lebih lama daripada kerja itu sendiri. Jelaskan tanggungjawab sekarang.
Cuti juga perlu diambil kira. Pelan tidak boleh mengandaikan semua orang sentiasa ada.
Saya setuju jika makluman bertulis juga menyatakan perkara sama. Keputusan bersyarat bukan janji pasti.
Baik, saya sudah betulkan nota. Tadi saya melihat draf lama dengan angka dan tarikh berbeza.
Kita teruskan. Soalan yang belum selesai kekal dalam senarai dan makluman seterusnya gunakan pelan yang dibetulkan.
'''),
'fil':pack('Ana|Miguel|Liza|Paolo|Maya|Carlo|Nina|Ramon','Hunyo','''
Bago tayo magpatuloy, kailangan ng malinaw na pasya tungkol sa {item}. Ihiwalay natin ang kumpirmado sa mungkahi.
Ang dating petsa para sa {item} ay {old_date}. Nakasalalay iyon sa impormasyong hindi dumating sa oras.
Ano ang nagbago mula sa huling usapan? Gastos ba ang problema o iskedyul?
Tapos na ang pangunahing gawain, pero may pagkakaiba sa {units} kaso sa pagsusuri. Hindi buong proyekto ang apektado.
Mungkahi ko, panatilihin ang dating petsa at tapusin ang ibang pagsusuri pagkatapos. Alam kong hindi ito perpekto.
Hindi ako sang-ayon. Hindi puwedeng mangakong tapos na kung walang pagsusuri. Bawasan na lang natin ang saklaw.
Puwedeng bawasan ang saklaw ng {item} pero panatilihin ang pagsusuri. Sa susunod na yugto ang natitira.
Nakasulat pa sa draft ang {old_amount}. Buong gastos na ba iyon o bahagi pa lang?
Lumang halaga iyon. Ang bagong limitasyon ay {amount}, kasama ang pagsusuri. Walang aprubadong dagdag pa rito.
Puwede bang laktawan na lang ang huling pagsusuri? Kung hindi, ipapaliwanag na naman natin ang paglipat ng petsa.
Hindi. Tinatanggihan ang paglaktaw sa pagsusuri. Kailangan pa rin iyon bago ituloy ang {item}.
Ihahanda ko ang binagong plano para sa {item} bago ang {date}, kasama ang saklaw, gastos at mga dokumento para sa pagsusuri.
Naisulat kong si {other} ang may hawak ng plano. Tama ba, o mali ang pangalan ko?
Si {owner}, hindi si {other}, ang responsable. Tutulong si {other} sa pagsusuri, pero kay {owner} ang plano at takdang oras.
Napagkasunduan ang mas maliit na saklaw ng {item} at limitasyong {amount}. Itutuloy lamang pagkatapos ng pagsusuri.
Bukas pa ang tanong kung kailangan ng dagdag na tulong para sa {item}. Kulang ang impormasyon ngayon.
Pakisabi ulit ang petsa. Naputol ang tunog noon at ayokong maipasa ang lumang iskedyul.
Ang bagong petsa ay {date}. Hindi na pangako ang dating {old_date}.
Noong nagmadali tayo, mas tumagal ang pagpapasa kaysa sa mismong trabaho. Linawin na natin ang responsibilidad ngayon.
Isama rin natin ang mga nakabakasyon. Hindi puwedeng umasa ang plano na laging narito ang lahat.
Sang-ayon ako kung ganoon din ang nakasulat na balita. Huwag gawing siguradong pangako ang pasyang may kondisyon.
Sige, inayos ko na ang mga tala. Lumang draft ang tinitingnan ko, kaya iba ang halaga at petsa.
Tuloy tayo. Iwan sa listahan ang bukas na tanong at gamitin ang naayos na plano sa susunod na balita.
'''),
'vi':pack('Lan|Minh|Hương|Tuấn|Mai|Dũng|Linh|Nam','tháng sáu','''
Trước khi chuyển tiếp, ta cần thống nhất về {item}. Hãy tách quyết định đã xác nhận khỏi đề xuất.
Mốc cũ của {item} là {old_date}. Mốc đó phụ thuộc vào dữ liệu mà ta không nhận được đúng hạn.
Từ lần trao đổi trước có gì thay đổi? Vấn đề nằm ở chi phí hay tiến độ?
Công việc chính đã xong, nhưng kiểm tra phát hiện sai lệch ở {units} trường hợp. Không phải toàn bộ dự án.
Tôi đề nghị giữ ngày cũ và hoàn thành các kiểm tra còn lại sau đó. Tôi biết cách này chưa lý tưởng.
Tôi không đồng ý. Chưa kiểm tra thì không thể hứa đã hoàn tất. Nên thu hẹp phạm vi hơn.
Ta có thể giảm phạm vi của {item} nhưng vẫn giữ kiểm tra. Phần còn lại chuyển sang giai đoạn sau.
Bản nháp vẫn ghi {old_amount}. Đó là tổng chi phí hay chỉ một phần?
Đó là số cũ. Giới hạn mới là {amount}, đã gồm kiểm tra. Chưa có phê duyệt chi vượt mức này.
Có thể bỏ lần kiểm tra cuối không? Nếu không, ta lại phải giải thích việc dời lịch.
Không. Đề xuất bỏ kiểm tra bị bác bỏ. Kiểm tra vẫn là điều kiện để tiếp tục {item}.
Tôi sẽ chuẩn bị kế hoạch sửa đổi cho {item} trước {date}, gồm phạm vi, chi phí và tài liệu kiểm tra.
Tôi ghi {other} phụ trách kế hoạch. Đúng không, hay tôi ghi nhầm tên?
Người phụ trách là {owner}, không phải {other}. {other} hỗ trợ kiểm tra, còn kế hoạch và hạn chót thuộc về {owner}.
Ta thống nhất phạm vi nhỏ hơn cho {item} và giới hạn {amount}. Chỉ tiếp tục sau khi kiểm tra.
Vẫn chưa quyết định có cần thêm người hỗ trợ {item} hay không. Hôm nay chưa đủ thông tin.
Nhắc lại ngày giúp tôi nhé. Đúng lúc đó âm thanh bị ngắt, tôi không muốn truyền ngày cũ.
Ngày mới là {date}. Ngày cũ {old_date} không còn là cam kết nữa.
Lần trước làm gấp, việc bàn giao còn lâu hơn việc thực hiện. Lần này nên làm rõ trách nhiệm từ đầu.
Ta cũng phải tính ngày nghỉ. Kế hoạch không thể dựa vào việc ai cũng luôn có mặt.
Tôi đồng ý nếu thông báo bằng văn bản nói đúng như vậy. Đừng biến quyết định có điều kiện thành lời hứa chắc chắn.
Hiểu rồi, tôi đã sửa ghi chú. Tôi xem nhầm bản cũ nên số tiền và ngày khác nhau.
Chuyển tiếp nhé. Giữ câu hỏi chưa giải quyết trong danh sách và dùng kế hoạch đã sửa cho lần cập nhật tới.
'''),
'th':pack('มาลี|สมชาย|อร|นนท์|พร|ชัย|นิด|กิตติ','มิถุนายน','''
ก่อนคุยเรื่องต่อไป เราต้องตกลงเรื่อง {item} ให้ชัด แยกสิ่งที่ยืนยันแล้วออกจากข้อเสนอนะคะ
กำหนดเดิมของ {item} คือ {old_date} แต่ต้องได้ข้อมูลก่อน ซึ่งเราไม่ได้รับทันเวลา
จากครั้งก่อนมีอะไรเปลี่ยนไปบ้าง ปัญหาอยู่ที่ค่าใช้จ่ายหรือกำหนดการ
งานหลักเสร็จแล้ว แต่ตรวจพบความแตกต่างใน {units} กรณี ไม่ได้มีปัญหาทั้งโครงการ
ผมเสนอให้คงวันเดิมไว้ แล้วค่อยตรวจส่วนที่เหลือทีหลัง รู้ว่าไม่ใช่ทางเลือกที่ดีที่สุด
ไม่เห็นด้วยค่ะ ถ้ายังไม่ตรวจ เรารับปากว่าเสร็จไม่ได้ ลดขอบเขตงานน่าจะดีกว่า
เราลดขอบเขตของ {item} ได้ แต่ยังต้องตรวจ ส่วนที่เหลือค่อยทำในระยะถัดไป
ในร่างยังเขียนว่า {old_amount} เป็นยอดรวมทั้งหมดหรือแค่บางส่วน
นั่นคือยอดเดิมค่ะ เพดานใหม่คือ {amount} รวมค่าตรวจแล้ว ยังไม่ได้อนุมัติให้เกินนี้
ข้ามการตรวจครั้งสุดท้ายได้ไหม ไม่อย่างนั้นต้องอธิบายเรื่องเลื่อนกำหนดอีก
ไม่ได้ค่ะ ไม่รับข้อเสนอให้ข้ามการตรวจ การตรวจยังเป็นเงื่อนไขของ {item}
ผมจะทำแผนแก้ไขของ {item} ภายใน {date} ระบุขอบเขต ค่าใช้จ่าย และเอกสารที่ใช้ตรวจ
ผมจดว่า {other} รับผิดชอบแผน ถูกไหม หรือจดชื่อผิด
ผู้รับผิดชอบคือ {owner} ไม่ใช่ {other} ส่วน {other} ช่วยตรวจ แต่แผนและกำหนดเป็นความรับผิดชอบของ {owner}
ตกลงลดขอบเขตของ {item} และใช้เพดาน {amount} จะเดินหน้าต่อได้หลังตรวจเท่านั้น
ยังไม่ได้ข้อสรุปว่า {item} ต้องการคนช่วยเพิ่มไหม ข้อมูลวันนี้ยังไม่พอ
ช่วยทวนวันที่อีกครั้งได้ไหม ตอนนั้นเสียงขาด ไม่อยากส่งต่อวันเก่า
วันใหม่คือ {date} ส่วนกำหนดเดิม {old_date} ไม่ใช่วันที่เรารับปากอีกแล้ว
ครั้งก่อนเรารีบจนการส่งมอบใช้เวลานานกว่างานเอง คราวนี้กำหนดความรับผิดชอบให้ชัดก่อน
ต้องเผื่อวันลาด้วย แผนไม่ควรตั้งอยู่บนสมมติฐานว่าทุกคนว่างตลอด
เห็นด้วยถ้าข้อความที่ส่งเป็นลายลักษณ์อักษรตรงกัน อย่าเปลี่ยนมติแบบมีเงื่อนไขให้เป็นคำรับปากแน่นอน
เข้าใจแล้ว แก้บันทึกแล้วค่ะ เมื่อกี้ดูร่างเก่า ตัวเลขกับวันที่เลยไม่ตรง
ไปเรื่องต่อไปกัน เก็บคำถามที่ยังไม่จบไว้ และใช้แผนที่แก้แล้วในการแจ้งครั้งหน้า
'''),
'tr':pack('Ayşe|Mehmet|Elif|Emre|Zeynep|Can|Deniz|Murat','Haziran','''
Devam etmeden {item} konusunda karar vermeliyiz. Kesinleşen kararları önerilerden ayıralım.
Önceki tarih {old_date} idi. Bu tarih, zamanında gelmeyen bilgilere bağlıydı.
Son görüşmeden beri ne değişti? Sorun maliyet mi, takvim mi?
Ana iş tamamlandı, ama kontrolde {units} vakada fark bulundu. Bütün proje etkilenmiş değil.
Eski tarihi koruyup kalan kontrolleri sonra yapmayı öneriyorum. İdeal olmadığını biliyorum.
Katılmıyorum. Kontrol olmadan tamamlandığını söyleyemeyiz. Kapsamı daraltmak daha doğru.
{item} kapsamını azaltabiliriz ama kontrolü koruyalım. Kalan kısmı sonraki aşamada yaparız.
Taslakta hâlâ {old_amount} yazıyor. Bu toplam maliyet mi, sadece bir kısmı mı?
O eski tutar. Kontrol dahil yeni üst sınır {amount}. Daha fazlasına onay verilmedi.
En azından son kontrolü atlayamaz mıyız? Yoksa tarih değişikliğini yine açıklamamız gerekecek.
Hayır. Kontrolü atlama önerisi reddedildi. {item} için kontrol hâlâ bir ön koşul.
{item} için düzeltilmiş planı {date} tarihine kadar hazırlayacağım. Kapsam, maliyet ve kontrol belgeleri yer alacak.
Planın sorumlusu olarak {other} yazmışım. Doğru mu, yoksa ismi mi karıştırdım?
Sorumlu {owner}, {other} değil. {other} kontrole yardım ediyor, ama plan ve süre {owner} sorumluluğunda.
{item} için daha dar kapsam ve {amount} sınırında anlaştık. Devam etmek kontrol sonucuna bağlı.
{item} için ek destek gerekip gerekmediği açık kaldı. Bugün karar verecek kadar bilgi yok.
Tarihi tekrar eder misin? Tam o sırada ses kesildi, eski tarihi iletmek istemiyorum.
Yeni tarih {date}. Eski tarih olan {old_date} artık verdiğimiz bir söz değil.
Geçen sefer acele ettik ve devir işi asıl işten uzun sürdü. Sorumlulukları şimdiden netleştirelim.
İzinleri de hesaba katmalıyız. Plan, herkesin her zaman hazır olmasına bağlı olamaz.
Yazılı bilgilendirmede de aynısı söylenirse katılıyorum. Koşullu kararı kesin söze çevirmeyelim.
Anladım, notları düzelttim. Eski taslağa bakıyormuşum, tutar ve tarih bu yüzden farklıydı.
Devam edelim. Açık soruyu listede tutalım ve sonraki güncellemede düzeltilmiş planı kullanalım.
'''),
}
