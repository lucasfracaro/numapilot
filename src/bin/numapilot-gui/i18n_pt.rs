//! English → Portuguese table (texts with values in the middle use `tf!` in the code itself).

pub const PT: &[(&str, &str)] = &[
    ("Programs", "Programas"),
    ("Startup", "Inicialização"),
    ("Services", "Serviços"),
    ("Map & measurements", "Mapa e medições"),
    ("Saved choices", "Escolhas salvas"),
    ("Graphics cards", "Placas de vídeo"),
    ("Machine", "Máquina"),
    ("Lab", "Laboratório"),
    ("AI server", "Servidor de IA"),
    ("Settings", "Configurações"),
    ("Tests", "Testes"),
    (
        "What is open now and which processor and card each one runs on",
        "O que está aberto agora e em qual processador e placa cada um roda",
    ),
    (
        "Everything that starts with the system: login programs and services",
        "Tudo que abre quando o sistema liga: programas do login e serviços",
    ),
    (
        "System and user services: start, stop, restart and read the log",
        "Serviços do sistema e do usuário: iniciar, parar, reiniciar e ver o log",
    ),
    (
        "Cores, memory per socket and performance measurements",
        "Núcleos, memória por socket e medições de desempenho",
    ),
    (
        "Per-program choices applied every time it starts",
        "Escolhas por programa que valem toda vez que ele abrir",
    ),
    (
        "Fan, power, clock and undervolt of each card",
        "Ventoinha, potência, clock e undervolt de cada placa",
    ),
    (
        "Power mode, processor, laptop and hardware health",
        "Modo de energia, processador, notebook e saúde do hardware",
    ),
    (
        "Containers with ROCm, Jupyter and IDEs",
        "Containers com ROCm, Jupyter e IDEs",
    ),
    (
        "Local models with llama.cpp",
        "Modelos locais com llama.cpp",
    ),
    (
        "Sunshine and Moonlight: play from another device",
        "Sunshine e Moonlight: jogar de outro aparelho",
    ),
    (
        "Other PCs, notifications, updates and virtual machine",
        "Outros PCs, avisos, atualização e máquina virtual",
    ),
    (
        "Manual test script, self-test and a report to send",
        "Roteiro para testar o programa, autoteste e relatório para enviar",
    ),
    ("Service running", "Serviço ativo"),
    ("Service stopped", "Serviço parado"),
    ("Start the service", "Ativar o serviço"),
    (
        "The numapilot service is not running: choices are not applied.",
        "O serviço do numapilot não está rodando: as escolhas não são aplicadas.",
    ),
    ("Last command", "Último comando"),
    ("Theme", "Tema"),
    ("System", "Sistema"),
    ("Light", "Claro"),
    ("Dark", "Escuro"),
    ("Memory", "Memória"),
    ("Name", "Nome"),
    ("Disk", "Disco"),
    ("GPU memory", "Memória da GPU"),
    ("Card", "Placa"),
    ("Adjust…", "Ajustar…"),
    ("End", "Encerrar"),
    ("Force stop", "Forçar parada"),
    ("Expand all", "Abrir tudo"),
    ("Collapse all", "Fechar tudo"),
    ("Only mine", "Só os meus"),
    ("🔍 Find installed app…", "🔍 Buscar app instalado…"),
    ("Find installed app", "Buscar app instalado"),
    ("Applications", "Aplicativos"),
    ("Session and system", "Sessão e sistema"),
    ("programs running", "programas em execução"),
    ("Menu tabs", "Abas do menu"),
    ("Language", "Idioma"),
    ("no video encoding", "sem codificação de vídeo"),
    (
        "install libva-utils to check",
        "instale libva-utils para verificar",
    ),
    ("note (optional)", "nota (opcional)"),
    (
        "What happened? (goes into the report)",
        "O que aconteceu? (aparece no relatório)",
    ),
    ("Failed", "Falhou"),
    ("Skipped", "Pulei"),
    (
        "Only the tests that apply to this machine are shown. Answers are saved; mark and write a note when something goes wrong.",
        "Só aparecem os testes que valem para esta máquina. As respostas ficam salvas; marque e escreva uma nota quando algo der errado.",
    ),
    ("Clear answers", "Limpar respostas"),
    (
        "A file with your answers, the last self-test and the end of the logs, to send.",
        "Um arquivo com suas respostas, o último autoteste e o final dos registros, para enviar.",
    ),
    ("Generate report", "Gerar relatório"),
    ("Self-test", "Autoteste"),
    (
        "Tests every part from the command line (read-only, changes nothing). Takes a minute or two.",
        "Testa todas as partes pela linha de comando (só lê, não muda nada). Leva um ou dois minutos.",
    ),
    ("Run the self-test", "Rodar o autoteste"),
    ("SKIPPED", "PULEI"),
    ("FAILED", "FALHOU"),
    (
        "The files show up in the backup folder.",
        "Os arquivos aparecem na pasta de cópias.",
    ),
    (
        "Use \"Save from all\" in Other PCs.",
        "Use \"Guardar de todos\" em Outros PCs.",
    ),
    ("Settings backup", "Cópia das configurações"),
    ("Other", "Outros"),
    (
        "The notification arrives on the phone.",
        "O aviso chega no celular.",
    ),
    (
        "Set up ntfy and send the test.",
        "Configure o ntfy e envie o teste.",
    ),
    ("Phone notifications", "Avisos no celular"),
    (
        "It shows as online; the wake command turns the PC on.",
        "Ele aparece online; o comando de ligar acorda o PC.",
    ),
    (
        "Add another PC's link and use \"wake over the network\" with it turned off.",
        "Adicione o link de outro PC e use \"ligar pela rede\" com ele desligado.",
    ),
    ("Other PCs", "Outros PCs"),
    ("It connects and the game opens.", "Conecta e o jogo abre."),
    (
        "Turn on Sunshine and connect with Moonlight; open a game from the phone.",
        "Ligue o Sunshine e conecte pelo Moonlight; abra um jogo pelo celular.",
    ),
    (
        "The container starts and Jupyter opens in the browser.",
        "O container sobe e o Jupyter abre no navegador.",
    ),
    (
        "Create a container and open Jupyter.",
        "Crie um container e abra o Jupyter.",
    ),
    (
        "It answers; \"Tune better\" finishes and suggests settings.",
        "Responde; \"Ajustar melhor\" termina e sugere configurações.",
    ),
    (
        "Start a small model and send a question.",
        "Inicie um modelo pequeno e mande uma pergunta.",
    ),
    (
        "It connects using software encoding, with a stable image.",
        "Conecta usando a codificação por software, com imagem estável.",
    ),
    (
        "Turn on Sunshine from numapilot and connect with Moonlight.",
        "Ligue o Sunshine pelo numapilot e conecte pelo Moonlight.",
    ),
    (
        "Streaming without a video encoder",
        "Streaming sem codificador de vídeo",
    ),
    (
        "Shows the active cores and CUs expected for the unlock.",
        "Mostra os núcleos e CUs ativos esperados para o desbloqueio.",
    ),
    ("Run the self-test.", "Rode o autoteste."),
    ("Cores and CUs", "Núcleos e CUs"),
    (
        "After rebooting, the self-test shows the new value under \"Memória da GPU (TTM)\".",
        "Depois de reiniciar, o autoteste mostra o valor novo em \"Memória da GPU (TTM)\".",
    ),
    (
        "Change the GPU memory and reboot.",
        "Mude a memória da GPU e reinicie.",
    ),
    (
        "The community apps show up with install/open.",
        "Os apps da comunidade aparecem com instalar/abrir.",
    ),
    (
        "In Graphics cards, look at the BC-250 section.",
        "Em Placas de vídeo, veja a seção da BC-250.",
    ),
    ("Community apps", "Apps da comunidade"),
    (
        "The laptop responds (fan/profile change) and the value stays saved.",
        "O notebook responde (ventoinha/perfil mudam) e o valor fica salvo.",
    ),
    (
        "Change the Acer profile/fan in Machine. If nothing shows up, run numapilot selftest: the Acer item shows why.",
        "Mude o perfil/ventoinha da Acer em Máquina. Se não aparece nada, rode numapilot selftest: o item Acer mostra o motivo.",
    ),
    ("Acer controls", "Controles da Acer"),
    ("Laptop", "Notebook"),
    (
        "The new value shows in ryzenadj and is still there after a few minutes.",
        "O valor novo aparece no ryzenadj e continua depois de alguns minutos.",
    ),
    (
        "Change the STAPM limit in Machine and run: sudo ryzenadj --info",
        "Mude o limite STAPM em Máquina e rode: sudo ryzenadj --info",
    ),
    ("Ryzen limits", "Limites do Ryzen"),
    (
        "All of them show an image; changing ports does not leave the displays flickering.",
        "Todas dão imagem; trocar de porta não deixa as telas piscando.",
    ),
    (
        "Connect monitors to HDMI and USB-C/Thunderbolt; move one to another port.",
        "Ligue monitores no HDMI e no USB-C/Thunderbolt; troque um de porta.",
    ),
    ("External displays", "Telas externas"),
    (
        "Nothing that starts with the system shows up holding the NVIDIA.",
        "Nada que abre com o sistema aparece segurando a NVIDIA.",
    ),
    (
        "After logging in, run: numapilot hybrid",
        "Depois de entrar na sessão, rode: numapilot hybrid",
    ),
    (
        "Login programs on the integrated GPU",
        "Programas do login na integrada",
    ),
    (
        "Lists only programs that really use the NVIDIA (no systemd, kwin, etc.).",
        "Lista só programas que realmente usam a NVIDIA (sem systemd, kwin, etc.).",
    ),
    ("Run: numapilot hybrid", "Rode: numapilot hybrid"),
    ("Who is holding the NVIDIA", "Quem segura a NVIDIA"),
    (
        "On battery the NVIDIA turns off (numapilot hybrid: \"desligada\"); on AC power it comes back.",
        "Na bateria a NVIDIA desliga (numapilot hybrid: \"desligada\"); na tomada volta.",
    ),
    (
        "Auto mode. On battery, close whatever uses the NVIDIA; then plug in the charger.",
        "Modo auto. Na bateria, feche o que usa a NVIDIA; depois ligue na tomada.",
    ),
    ("NVIDIA on automatic", "NVIDIA no automático"),
    (
        "Within 10 s numapilot shows \"on battery\" and then \"on AC power\".",
        "Em até 10 s o numapilot mostra \"na bateria\" e depois \"na tomada\".",
    ),
    (
        "Unplug the charger; then plug it back in.",
        "Tire o carregador; depois ligue de novo.",
    ),
    ("AC power and battery", "Tomada e bateria"),
    (
        "Disks, memory and cards show a status (no \"never collected\").",
        "Discos, memória e placas aparecem com estado (sem \"nunca coletado\").",
    ),
    (
        "Wait 1 minute after starting the service and open the Health section.",
        "Espere 1 minuto depois de iniciar o serviço e abra a seção Saúde.",
    ),
    ("Hardware health", "Saúde do hardware"),
    (
        "Today/month start counting and grow with usage.",
        "Hoje/mês começam a contar e crescem com o uso.",
    ),
    (
        "Enter the kWh price and wait a few minutes.",
        "Informe o preço do kWh e espere alguns minutos.",
    ),
    ("Energy and cost", "Energia e custo"),
    (
        "At that time the mode switches by itself; after the end, it goes back.",
        "Na hora, o modo troca sozinho; depois do fim, volta.",
    ),
    (
        "Create a schedule that starts 2 minutes from now.",
        "Crie um horário que começa daqui a 2 minutos.",
    ),
    ("Scheduled mode", "Modo por horário"),
    (
        "The governor and the energy preference change (numapilot selftest shows it); Normal restores what it was.",
        "O governador e a preferência de energia mudam (numapilot selftest mostra); Normal volta ao que era.",
    ),
    (
        "Switch between Game, AI, Quiet and Normal.",
        "Troque entre Jogo, IA, Silencioso e Normal.",
    ),
    ("Machine modes", "Modos da máquina"),
    (
        "Under load, the card's clock stays under the chosen value; Undo restores the factory value.",
        "Com carga, o clock da placa não passa do valor escolhido; Desfazer volta ao de fábrica.",
    ),
    (
        "On the AMD card, under \"Clock and voltage\", check customize, lower \"GPU max\" and apply; then Undo.",
        "Na placa AMD, em \"Clock e voltagem\", marque personalizar, baixe \"GPU máx.\" e aplique; depois Desfazer.",
    ),
    ("Clock and voltage (AMD)", "Clock e voltagem (AMD)"),
    ("Cards", "Placas"),
    (
        "After rebooting, the card's clock and voltage controls become available.",
        "Depois de reiniciar, os controles de clock e voltagem da placa ficam disponíveis.",
    ),
    (
        "Use \"Enable OverDrive\" and reboot the PC.",
        "Use \"Ativar OverDrive\" e reinicie o PC.",
    ),
    (
        "The value shows as applied and resets by itself after restarting the service.",
        "O valor aparece aplicado e volta sozinho depois de reiniciar o serviço.",
    ),
    (
        "Apply a small offset (e.g. +50 MHz) and restart the service.",
        "Aplique um deslocamento pequeno (ex.: +50 MHz) e reinicie o serviço.",
    ),
    ("Clock offset (NVIDIA)", "Deslocamento de clock (NVIDIA)"),
    (
        "It picks a load program with 80% usage or more, shows clock and usage, finishes and saves; the terminal closes with Enter.",
        "Escolhe um programa de carga com 80% de uso ou mais, mostra clock e uso, termina e grava; o terminal fecha com Enter.",
    ),
    (
        "On AC power, use \"Find the undervolt automatically\" and leave the PC idle.",
        "Na tomada, use \"Achar o undervolt sozinho\" e deixe o PC parado.",
    ),
    ("Automatic undervolt", "Undervolt automático"),
    (
        "The watts at the top stay under the limit; Undo restores the default.",
        "Os watts no topo não passam do limite; Desfazer volta ao padrão.",
    ),
    (
        "Lower a card's power limit and run a game. If the card does not show this control, the vendor locks it: mark Skipped.",
        "Baixe o limite de potência de uma placa e rode um jogo. Se a placa não mostra esse controle, o fabricante trava: marque Pulei.",
    ),
    ("Power limit", "Limite de potência"),
    (
        "The fan follows the curve; Undo goes back to the card's automatic mode.",
        "A ventoinha segue a curva; Desfazer volta para o automático da placa.",
    ),
    (
        "In Graphics cards, choose \"curve\", apply and then Undo.",
        "Em Placas de vídeo, escolha \"curva\", aplique e depois Desfazer.",
    ),
    ("Fan curve", "Curva da ventoinha"),
    (
        "The table fills in with no error.",
        "A tabela é preenchida sem erro.",
    ),
    ("Click \"Measure now\".", "Clique em \"Medir agora\"."),
    ("Measure latency and bandwidth", "Medir latência e banda"),
    ("Map", "Mapa"),
    (
        "Under \"Allocations on the other socket\" this program does not increase remote allocations.",
        "Em \"Alocações no outro socket\" esse programa não aumenta as alocações remotas.",
    ),
    (
        "In a program's Adjust…, choose to use only its socket's memory.",
        "Em Ajustar… de um programa, escolha usar só a memória do socket dele.",
    ),
    ("Socket-only memory", "Memória só do socket"),
    (
        "It follows the real temperature (up under load, down when idle).",
        "Acompanha a temperatura real (sobe com carga, desce parado).",
    ),
    (
        "Watch the temperature chart under load and then idle.",
        "Olhe o gráfico de temperatura com carga e depois parado.",
    ),
    ("Processor temperature", "Temperatura do processador"),
    (
        "The charts of the threads in use go up; hovering shows the core and the frequency.",
        "Os gráficos dos threads em uso sobem; ao passar o mouse mostra o núcleo e a frequência.",
    ),
    (
        "Open Map & measurements → Per-thread chart and run something heavy.",
        "Abra Mapa e medições → Gráfico por thread e rode algo pesado.",
    ),
    ("Per-thread chart", "Gráfico por thread"),
    (
        "The games' launch options change and go back to what they were.",
        "As opções de inicialização dos jogos mudam e voltam ao que eram.",
    ),
    (
        "Use \"Apply to all games\" with Steam closed; then \"Remove from all\".",
        "Use \"Aplicar em todos os jogos\" com a Steam fechada; depois \"Remover de todos\".",
    ),
    ("Steam games", "Jogos da Steam"),
    (
        "The program reopens and starts using the chosen card (its usage shows there).",
        "O programa reabre e passa a usar a placa escolhida (o uso aparece nela).",
    ),
    (
        "In Adjust…, pick another card and use Reopen.",
        "Em Ajustar…, escolha outra placa e use Reabrir.",
    ),
    ("Choose the card", "Escolher a placa"),
    (
        "The program does not go past the limit (the excess goes to swap); the PC does not freeze.",
        "O programa não passa do limite (o excedente vai para o swap); o PC não trava.",
    ),
    (
        "In Adjust…, check \"Limit the program's memory to\" 2 GB and use the program.",
        "Em Ajustar…, marque \"Limitar a memória do programa a\" 2 GB e use o programa.",
    ),
    ("Memory limit", "Limite de memória"),
    (
        "In the Map (per-thread chart) the yellow dot shows on those cores and the load stays on them.",
        "No Mapa (gráfico por thread) aparece o ponto amarelo nesses núcleos e o uso fica neles.",
    ),
    (
        "In Adjust…, pick specific cores and save.",
        "Em Ajustar…, escolha núcleos específicos e salve.",
    ),
    ("Choose cores", "Escolher núcleos"),
    (
        "End closes it; Force stop asks for confirmation and closes it. kwin, plasmashell and Xwayland stay under \"Session and system\" and the buttons refuse.",
        "Encerrar fecha; Forçar parada pede confirmação e fecha. kwin, plasmashell e Xwayland ficam em \"Sessão e sistema\" e os botões não deixam.",
    ),
    (
        "Open one of your programs (e.g. Dolphin), select it and use End; open it again and use Force stop.",
        "Abra um programa seu (ex.: Dolphin), selecione e use Encerrar; abra de novo e use Forçar parada.",
    ),
    ("End and Force stop", "Encerrar e Forçar parada"),
    (
        "The order changes; CPU, memory and GPU of a game match Mission Center or MangoHud.",
        "A ordem muda; CPU, memória e GPU de um jogo batem com o Mission Center ou o MangoHud.",
    ),
    (
        "Click the Name, CPU, Memory and GPU headers (again to reverse); search by name and by PID.",
        "Clique nos títulos Nome, CPU, Memória e GPU (de novo inverte); busque por nome e por PID.",
    ),
    ("Sort and search", "Ordenar e buscar"),
    (
        "It shows up in Programs within 5 seconds, with the socket and the card it is using.",
        "Ele aparece em Programas em até 5 segundos, com o socket e a placa que está usando.",
    ),
    (
        "Open any program (browser, game).",
        "Abra um programa qualquer (navegador, jogo).",
    ),
    ("Program list", "Lista de programas"),
    (
        "Shows what was done (service, program and commands) with date and time.",
        "Mostra o que foi feito (serviço, programa e comandos) com data e hora.",
    ),
    (
        "In the terminal: numapilot log",
        "No terminal: numapilot log",
    ),
    ("Log", "Registro"),
    ("General", "Geral"),
    (
        "\"Service stopped\" shows up with the button; the button asks for the password and it goes back to \"Service running\".",
        "Aparece \"Serviço parado\" com o botão; o botão pede a senha e volta para \"Serviço ativo\".",
    ),
    (
        "In the terminal: sudo systemctl stop numapilot. Then use the \"Start the service\" button.",
        "No terminal: sudo systemctl stop numapilot. Depois use o botão \"Ativar o serviço\".",
    ),
    ("Service stopped and restart", "Serviço parado e religar"),
    (
        "The values go up, the bars slide without jumping and match another monitor (Mission Center, nvtop).",
        "Os valores sobem, as barras deslizam sem pular e batem com outro monitor (Mission Center, nvtop).",
    ),
    (
        "Open a game or benchmark and watch the CPU, memory and card tiles.",
        "Abra um jogo ou benchmark e olhe os cartões de CPU, memória e placas.",
    ),
    ("Top meters", "Medidores do topo"),
    (
        "The theme changes right away and stays as chosen after reopening.",
        "O tema muda na hora e continua o escolhido depois de reabrir.",
    ),
    (
        "Switch to Light and Dark in the side menu, close and open again.",
        "Troque para Claro e Escuro no menu lateral, feche e abra de novo.",
    ),
    (
        "The window opens. There is only one numapilot icon in the tray.",
        "A janela abre. Só existe um ícone do numapilot na bandeja.",
    ),
    (
        "Turn on the tray icon (Settings), close the window and click the icon.",
        "Ligue o ícone na bandeja (Configurações), feche a janela e clique no ícone.",
    ),
    ("Tray icon", "Ícone na bandeja"),
    (
        "The window that was already open comes to the front; no second one opens.",
        "A janela que já estava aberta vem para frente; não abre outra.",
    ),
    (
        "With the window open, open numapilot again from the menu.",
        "Com a janela aberta, abra o numapilot de novo pelo menu.",
    ),
    ("Single window", "Uma janela só"),
    (
        "The window opens within a few seconds, in the system theme, with no error message.",
        "A janela abre em poucos segundos, no tema do sistema, sem aviso de erro.",
    ),
    (
        "Open numapilot from the system menu.",
        "Abra o numapilot pelo menu do sistema.",
    ),
    ("Open the program", "Abrir o programa"),
    (
        "Bring the memory to the right socket",
        "Trazer a memória para o socket certo",
    ),
    ("Apply", "Aplicar"),
    (
        "Apply right away: processors, cores, memory (except interleave), limit, priority and exclusive. \
                         Apply on reopen: graphics card, interleave and what the program sees.",
        "Valem na hora: processadores, núcleos, memória (exceto intercalar), limite, prioridade e exclusivo. \
                         Valem ao reabrir: placa de vídeo, intercalar e o que o programa enxerga.",
    ),
    ("socket only", "só do socket"),
    (
        "gamescope uses the same card chosen above. Good for running the game at a lower resolution and upscaling with FSR, or capping the FPS.",
        "O gamescope usa a mesma placa escolhida acima. Bom para rodar o jogo em resolução menor e ampliar com FSR, ou travar o FPS.",
    ),
    ("Upscale with FSR", "Ampliar com FSR"),
    ("Fullscreen", "Tela cheia"),
    ("FPS limit", "Limite de FPS"),
    ("Output resolution", "Resolução de saída"),
    ("unchecked = the display's", "desmarcado = a da tela"),
    ("Game resolution", "Resolução do jogo"),
    (
        "(not installed: sudo pacman -S gamescope)",
        "(não instalado: sudo pacman -S gamescope)",
    ),
    ("Open inside gamescope", "Abrir dentro do gamescope"),
    (
        "Click a core to use both threads; right-click = only 1 thread.",
        "Clique num núcleo para usar os 2 threads; botão direito = só 1 thread.",
    ),
    ("same GPU as the game", "mesma GPU do jogo"),
    ("same GPU", "mesma GPU"),
    ("video", "vídeo"),
    ("compute (AI/ROCm)", "computação (IA/ROCm)"),
    ("3D fullscreen (game)", "3D tela cheia (jogo)"),
    ("leave as is", "não mexer"),
    ("Card power profile:", "Perfil de energia da placa:"),
    ("Graphics card", "Placa de vídeo"),
    ("Quiet", "Silencioso"),
    ("AI", "IA"),
    ("Game", "Jogo"),
    ("do not switch", "não trocar"),
    (
        "While the program is open the machine stays in this mode (Machine tab) and goes back to the previous one when it closes.",
        "Enquanto o programa está aberto a máquina fica neste modo (aba Máquina) e volta ao anterior quando ele fecha.",
    ),
    ("Machine mode", "Modo da máquina"),
    (
        "The fewer cores, the higher the clock. This processor's table shows up with the service running (Intel only).",
        "Quanto menos núcleos, mais alto o clock. A tabela deste processador aparece com o serviço rodando (só Intel).",
    ),
    (
        "Turbo counts awake cores, not threads: the maximum clock is the same either way. \
                                     Unchecked, each program thread gets a whole core (better for most games). \
                                     Checked, it does better in programs that use every thread (compiling, rendering).",
        "O turbo conta núcleos acordados, não threads: o clock máximo é o mesmo com ou sem. \
                                     Desmarcado, cada thread do programa tem um núcleo inteiro (melhor para a maioria dos jogos). \
                                     Marcado, rende mais em programas que usam todos os threads (compilação, renderização).",
    ),
    (
        "use both threads of each core",
        "usar os dois threads de cada núcleo",
    ),
    ("Cores at turbo", "Núcleos no turbo"),
    (
        "While it runs: its cores do not enter the deep idle states and the socket's uncore \
                             stays at maximum (less micro-stutter). Tune it in Machine → Gaming latency.",
        "Enquanto ele roda: os núcleos dele não entram nos estados de economia profundos e o uncore do socket \
                             fica no máximo (menos travadinhas). Ajuste em Máquina → Latência em jogos.",
    ),
    ("low latency", "baixa latência"),
    (
        "While it runs, the rest of the system leaves the whole processor it is on. \
                                 With the other cores asleep, its cores clock higher (see the table in Map → Turbo). \
                                 Works best with few cores (2 to 8) and the deep idle states enabled.",
        "Enquanto ele roda, o resto do sistema sai do processador inteiro onde ele está. \
                                 Com os outros núcleos dormindo, o clock dos núcleos dele sobe mais (veja a tabela em Mapa → Turbo). \
                                 Funciona melhor com poucos núcleos (2 a 8) e os estados de economia profundos ligados.",
    ),
    ("maximum turbo", "turbo máximo"),
    (
        "While it runs, the rest of the system does not use these cores.",
        "Enquanto ele roda, o resto do sistema não usa esses núcleos.",
    ),
    ("exclusive cores", "núcleos exclusivos"),
    ("Priority", "Prioridade"),
    (
        "Usage cap adding up all cores: 50% = half a core, 200% = two cores. Good for background programs.",
        "Teto de uso somando todos os núcleos: 50% = meio núcleo, 200% = dois núcleos. Bom para programas de fundo.",
    ),
    ("Use at most", "Usar no máximo"),
    ("Processor limit", "Limite de processador"),
    (
        "Past the limit, the system takes memory away from it (to swap) instead of freezing the whole PC.",
        "Passou do limite, o sistema tira memória dele (vai para o swap) em vez de travar o PC todo.",
    ),
    (
        "Limit the program's memory to",
        "Limitar a memória do programa a",
    ),
    (
        "This PC has a single processor: all memory is already the closest to it (the local memory options show up on PCs with 2 processors).",
        "Este PC tem um processador só: toda a memória já é a mais próxima dele (as opções de memória local aparecem nos PCs com 2 processadores).",
    ),
    (
        "Proton/Wine games (WINE_CPU_TOPOLOGY), OpenMP, MKL, OpenBLAS, .NET, Java, Go...\n\
                             Keeps games and Easy Anti-Cheat from refusing to start for seeing too many cores. Applies on start.",
        "Jogos do Proton/Wine (WINE_CPU_TOPOLOGY), OpenMP, MKL, OpenBLAS, .NET, Java, Go...\n\
                             Evita jogos e o Easy Anti-Cheat recusarem abrir por ver núcleos demais. Vale ao abrir.",
    ),
    ("only the allowed cores", "só os núcleos liberados"),
    ("The program sees", "O programa enxerga"),
    ("pick the cores on the map", "escolher os núcleos no mapa"),
    (
        "Off: 1 thread per physical core. In games it usually gives more FPS and less stutter.",
        "Desligado: 1 thread por núcleo físico. Em jogos costuma dar mais FPS e menos travadas.",
    ),
    (
        "use hyper-threading (2 threads per core)",
        "usar hyper-threading (2 threads por núcleo)",
    ),
    ("limit to", "limitar a"),
    ("Cores", "Núcleos"),
    ("Processors", "Processadores"),
    ("All automatic", "Tudo automático"),
    (
        "4 cores and low priority: stays out of the way",
        "4 núcleos e prioridade baixa: não atrapalha o resto",
    ),
    (
        "at most one processor core and low priority: for launchers, sync tools and the like",
        "no máximo um núcleo de processador e prioridade baixa: para launchers, sincronizadores e afins",
    ),
    ("Background", "Segundo plano"),
    (
        "the GPU's whole socket, with both threads of each core, memory preferring local",
        "o socket da GPU inteiro, com os dois threads de cada núcleo, memória preferindo a local",
    ),
    ("Video editing", "Edição de vídeo"),
    (
        "4 physical cores alone on one processor: the rest of the system moves to the other socket and the clock rises to the highest turbo step (needs 2 processors; check Map → Turbo)",
        "4 núcleos físicos sozinhos num processador: o resto do sistema vai para o outro socket e o clock sobe ao degrau mais alto do turbo (precisa de 2 processadores; confira em Mapa → Turbo)",
    ),
    ("Maximum turbo", "Turbo máximo"),
    (
        "both sockets, interleaved memory (maximum bandwidth)",
        "os 2 sockets, memória intercalada (banda máxima)",
    ),
    ("Rendering", "Renderização"),
    (
        "compute GPU's socket, memory preferring local",
        "socket da GPU de computação, memória preferindo a local",
    ),
    (
        "GPU's socket, 16 physical cores, local memory only, high priority, reserved cores, the game sees only those cores",
        "socket da GPU, 16 núcleos físicos, memória só local, prioridade alta, núcleos reservados, o jogo enxerga só esses núcleos",
    ),
    ("Ready-made profiles:", "Perfis prontos:"),
    (
        "From the last 6 hours the program was open, one point per minute. Use it to pick the memory and processor limits with headroom over the peak.",
        "Das últimas 6 horas em que o programa esteve aberto, um ponto por minuto. Use para escolher o limite de memória e de processador com folga sobre o pico.",
    ),
    (
        "Open it from the menu shortcut (numapilot run) for the test to count.",
        "Abra pelo atalho do menu (numapilot run) para o teste valer.",
    ),
    ("Cancel", "Cancelar"),
    ("Start over", "Recomeçar"),
    ("Use the best", "Usar a melhor"),
    ("Open the game", "Abrir o jogo"),
    ("All tests finished.", "Todos os testes terminaram."),
    ("next", "próximo"),
    ("Average FPS", "FPS médio"),
    ("Setting", "Configuração"),
    ("Start the optimization", "Começar a otimização"),
    (
        "Needs MangoHud: sudo pacman -S mangohud",
        "Precisa do MangoHud: sudo pacman -S mangohud",
    ),
    (
        "Tests a few settings (cores, HT, socket, card), one each time you open the game, \
                     measures with MangoHud (average FPS and 1% low) and shows the best.",
        "Testa algumas configurações (núcleos, HT, socket, placa), uma a cada vez que você abre o jogo, \
                     mede com o MangoHud (FPS médio e 1% low) e mostra a melhor.",
    ),
    ("Optimize FPS", "Otimizar FPS"),
    (
        "To install from the interface, open the game through Steam with numapilot (the profile is tied to the appid).",
        "Para instalar pela interface, abra o jogo pela Steam com o numapilot (o perfil fica ligado ao appid).",
    ),
    (
        "Game folder not found in the Steam libraries.",
        "Pasta do jogo não encontrada nas bibliotecas da Steam.",
    ),
    (
        "Downloads the newest stable version and copies it to the executable's folder, keeping what belongs to the game",
        "Baixa a versão estável mais nova e copia para a pasta do executável, guardando o que for do jogo",
    ),
    ("Install in the game", "Instalar no jogo"),
    ("Reinstall (new version)", "Reinstalar (versão nova)"),
    ("Remove from the game", "Remover do jogo"),
    (
        "Do not use OptiScaler in online games: the anti-cheat may ban you.",
        "Não use o OptiScaler em jogos online: o anti-cheat pode banir.",
    ),
    (
        "For cards without official FSR 4 (RDNA 2/3.5, Intel, NVIDIA). Cards without INT8 do not get FSR 4.",
        "Para placas sem FSR 4 oficial (RDNA 2/3.5, Intel, NVIDIA). Placas sem INT8 não ganham FSR 4.",
    ),
    (
        "force FSR 4 INT8 (experimental)",
        "forçar FSR 4 INT8 (experimental)",
    ),
    (
        "show the FSR 4/DLSS watermark (to check)",
        "mostrar a marca do FSR 4/DLSS (para conferir)",
    ),
    (
        "Needs proton-cachyos or GE-Proton as the game's compatibility tool.",
        "Precisa do proton-cachyos ou do GE-Proton como ferramenta de compatibilidade do jogo.",
    ),
    ("None", "Nenhum"),
    ("does not move the memory", "não move a memória"),
    ("Free", "Livre"),
    (
        "more total bandwidth; good for rendering/AI on the CPU (applies on reopen)",
        "mais banda total; bom para render/IA na CPU (vale ao reabrir)",
    ),
    ("Interleave across sockets", "Intercalar entre sockets"),
    (
        "uses the other socket's only when it runs out",
        "usa a do outro socket só quando faltar",
    ),
    ("Prefer local", "Preferir a local"),
    (
        "lowest latency; if the socket's memory fills up, the program suffers",
        "menor latência; se a memória do socket encher, o programa sofre",
    ),
    ("Socket only (strict)", "Só do socket (estrito)"),
    (
        "numapilot decides (normally: memory of the cores' socket)",
        "o numapilot decide (normalmente: memória do socket dos núcleos)",
    ),
    ("Automatic", "Automático"),
    (
        "BC-250: with 16 GB, 14 GiB lets models up to ~14B load (BIOS with UMA at 512 MB). The AI server uses Vulkan and already sets GGML_VK_PREFER_HOST_MEMORY (+5 to 12% in generation).",
        "BC-250: com 16 GB, 14 GiB deixa os modelos de até ~14B carregarem (BIOS com UMA em 512 MB). O servidor de IA usa o Vulkan e já liga o GGML_VK_PREFER_HOST_MEMORY (+5 a 12% na geração).",
    ),
    ("default", "padrão"),
    (
        "Saves for the next boots and applies now (asks for the password)",
        "Grava para os próximos boots e aplica agora (pede a senha)",
    ),
    (
        "How much RAM the integrated GPU may use. The kernel default is half, which keeps large models from loading. Leave 1-2 GiB for the system.",
        "Quanto da RAM a GPU integrada pode usar. O padrão do kernel é a metade, o que impede modelos grandes de carregar. Deixe 1-2 GiB para o sistema.",
    ),
    ("Memory for the GPU (AI):", "Memória para a GPU (IA):"),
    ("Disabling OverDrive", "Desativando o OverDrive"),
    ("Enabling OverDrive", "Ativando o OverDrive"),
    ("Test the card selection", "Testar a escolha da placa"),
    ("Undo", "Desfazer"),
    (
        "After 1 min idle, the card stays at the lowest clock and voltage level until it is used again. Helps on the MI50s, which draw a lot when idle; the first request afterwards takes a fraction of a second longer.",
        "Parada há 1 min, a placa fica no nível mais baixo de clock e voltagem até voltar a ser usada. Ajuda nas MI50, que gastam bastante paradas; o primeiro pedido depois demora uma fração de segundo a mais.",
    ),
    ("Power saving when idle", "Economia quando parada"),
    ("Maximum frequency:", "Frequência máxima:"),
    ("card default", "padrão da placa"),
    ("Power:", "Potência:"),
    ("default curve", "curva padrão"),
    ("+ point", "+ ponto"),
    ("card's own (automatic)", "da placa (automática)"),
    (
        "(curve stored in the card's firmware)",
        "(curva gravada no firmware da placa)",
    ),
    ("Fan:", "Ventoinha:"),
    (
        "The driver does not expose fan/power controls for this card.",
        "O driver não expõe controles de ventoinha/potência para esta placa.",
    ),
    (
        "Asleep to save power: it wakes by itself when a program uses the card. The controls show up when it is on.",
        "Dormindo para economizar energia: acorda sozinha quando um programa usa a placa. Os controles aparecem com ela ligada.",
    ),
    (
        "The official NVIDIA driver is not loaded: no sensors or controls.",
        "O driver oficial da NVIDIA não está carregado: sem sensores nem controles.",
    ),
    (
        "Changes by itself depending on the program (Adjust… → Card power profile).",
        "Muda sozinho conforme o programa (Ajustar… → Perfil de energia da placa).",
    ),
    (
        "The generation drops when the card is idle (power saving); check it while in use.",
        "A geração cai quando a placa está parada (economia); confira com ela em uso.",
    ),
    (
        "Runs vulkaninfo and glxinfo the way a game would on this card and shows which card answered (Vulkan/DirectX and OpenGL).",
        "Roda o vulkaninfo e o glxinfo como um jogo rodaria nesta placa e mostra qual placa respondeu (Vulkan/DirectX e OpenGL).",
    ),
    ("Test this card's selection", "Testar a escolha desta placa"),
    ("History:", "Histórico:"),
    (
        "Temperature, fan and power of each card. The numapilot service applies them; \
             if it stops, the fans go back to the card's own control. Above 95 °C the fan goes to 100%.",
        "Temperatura, ventoinha e potência de cada placa. Quem aplica é o serviço numapilot; \
             se ele parar, as ventoinhas voltam para o controle da própria placa. Acima de 95 °C a ventoinha vai a 100%.",
    ),
    (
        "manual install (see the page)",
        "instalação manual (veja a página)",
    ),
    ("page", "página"),
    (
        "Downloads the code (git) into ~/.local/share/numapilot/comunidade and creates the commands in ~/.local/bin, in a terminal",
        "Baixa o código (git) em ~/.local/share/numapilot/comunidade e cria os comandos em ~/.local/bin, num terminal",
    ),
    ("Install", "Instalar"),
    (
        "Installs from the AUR (paru/yay) in a terminal; may ask for the password",
        "Instala pelo AUR (paru/yay) num terminal; pode pedir a senha",
    ),
    (
        "Installs from the AUR (the author's -git package) in a terminal; may ask for the password. Afterwards this button becomes \"Manage cores, CUs, frequencies\"",
        "Instala pelo AUR (pacote -git do autor) num terminal; pode pedir a senha. Depois este botão vira \"Gerenciar núcleos, CUs, frequências\"",
    ),
    (
        "Install BC250 Control Center",
        "Instalar o BC250 Control Center",
    ),
    (
        "Downloads and installs the maintainer's newest version (in a terminal; may ask for the password)",
        "Baixa e instala a versão mais nova de quem mantém (num terminal; pode pedir a senha)",
    ),
    ("Update", "Atualizar"),
    (
        "⚙ Manage cores, CUs, frequencies and voltages",
        "⚙ Gerenciar núcleos, CUs, frequências e voltagens",
    ),
    (
        "On the command line: numapilot board setup",
        "Na linha de comando: numapilot board setup",
    ),
    (
        "(section further down in this tab)",
        "(seção mais abaixo nesta aba)",
    ),
    ("Install (AUR)", "Instalar (AUR)"),
    ("Enable (asks for the password)", "Ligar (pede a senha)"),
    ("Community guide", "Guia da comunidade"),
    (
        "Installed from the maintainer's newest code (AUR -git or git); \"Update\" downloads it again.",
        "Instalados do código mais novo de quem mantém (AUR -git ou git); \"Atualizar\" baixa de novo.",
    ),
    (
        " (bc250_cc_write_mode=3): do not use the live unlock together with it",
        " (bc250_cc_write_mode=3): não use o desbloqueio ao vivo junto",
    ),
    ("GPU governor", "Governor da GPU"),
    (
        "Raise the clock and the voltage little by little and test the stability (e.g. Unigine Superposition) at each step.",
        "Suba o clock e a voltagem aos poucos e teste a estabilidade (ex.: Unigine Superposition) a cada passo.",
    ),
    (
        "Backs up the configuration, writes the new one and restarts the service; if it does not come up, the previous one is restored. Asks for the password.",
        "Faz backup da configuração, grava a nova e reinicia o serviço; se ele não subir, volta a anterior. Pede a senha.",
    ),
    (
        "Save and restart the governor",
        "Salvar e reiniciar o governor",
    ),
    (
        "Opens a terminal with paru/yay",
        "Abre um terminal com o paru/yay",
    ),
    ("Install the governor (AUR)", "Instalar o governor (AUR)"),
    (
        "No governor running: the GPU stays at a fixed clock.",
        "Nenhum governor rodando: a GPU fica com clock fixo.",
    ),
    ("Community ROCm", "ROCm da comunidade"),
    ("Official ROCm", "ROCm oficial"),
    ("frame generation (lsfg-vk)", "geração de quadros (lsfg-vk)"),
    ("FSR 4 (through vkd3d-proton)", "FSR 4 (via vkd3d-proton)"),
    ("up to date", "em dia"),
    ("Updating system and driver", "Atualizando sistema e driver"),
    (
        "On CachyOS the driver updates along with the system (pacman -Syu). Asks for the password.",
        "No CachyOS o driver atualiza junto com o sistema (pacman -Syu). Pede a senha.",
    ),
    ("NVIDIA driver", "Driver NVIDIA"),
    (
        "Updates the system and installs the right driver for the card's generation (through CachyOS's chwd). Asks for the password; then reboot.",
        "Atualiza o sistema e instala o driver certo para a geração da placa (pelo chwd do CachyOS). Pede a senha; depois reinicie.",
    ),
    ("Install official driver", "Instalar driver oficial"),
    ("Reinstall official driver", "Reinstalar driver oficial"),
    (
        "The installed driver is newer than the loaded one: reboot.",
        "O driver instalado é mais novo que o carregado: reinicie.",
    ),
    (
        "Using nouveau (the community's open driver): no performance for games.",
        "Usando o nouveau (driver aberto da comunidade): sem desempenho para jogos.",
    ),
    ("no", "não"),
    (
        "Undervolt on NVIDIA: lock the clock (e.g. 1800 MHz) and offset the curve (+100 to +200 MHz): same FPS using less power and heat. The \"Find automatically\" button tests the steps for you.",
        "Undervolt na NVIDIA: trave o clock (ex.: 1800 MHz) e desloque a curva (+100 a +200 MHz): mesmo FPS gastando e esquentando menos. O botão \"Achar sozinho\" testa os passos por você.",
    ),
    (
        "Without locking the maximum clock, the offset becomes an overclock (more clock, same voltage). For undervolt, lock the clock too.",
        "Sem travar o clock máximo, o deslocamento vira overclock (mais clock, mesma voltagem). Para undervolt, trave o clock também.",
    ),
    ("Memory:", "Memória:"),
    ("offset the curve", "deslocar a curva"),
    (
        "Shifts the clock/voltage curve up: with the maximum clock locked, the card reaches it at a lower voltage. Go up 15 MHz at a time and test in a game; if it hangs or shows artifacts, go back one step.",
        "Desloca a curva clock/voltagem para cima: com o clock máximo travado, a placa chega nele com voltagem menor. Suba de 15 em 15 MHz e teste em jogo; se travar ou der artefato, volte um passo.",
    ),
    ("Maximum clock:", "Clock máximo:"),
    (
        "needs vkmark: sudo pacman -S vkmark",
        "precisa do vkmark: sudo pacman -S vkmark",
    ),
    (
        "Tests step by step under load (1 min each) and saves the last stable one with 1 step of margin. Leave the PC idle; takes 10 to 30 min.",
        "Testa passo a passo com carga (1 min cada) e grava o último estável com 1 passo de margem. Deixe o PC parado; leva de 10 a 30 min.",
    ),
    (
        "Find the undervolt automatically",
        "Achar o undervolt sozinho",
    ),
    (
        "If the card hangs or the game closes by itself, go back one step. On reboot the service reapplies; unchecking \"customize\" restores the default.",
        "Se a placa travar ou o jogo fechar sozinho, volte um passo. Ao reiniciar o PC o serviço reaplica; desmarcar \"personalizar\" volta ao padrão.",
    ),
    (
        "Undervolt: same frequency with less voltage (less heat and power). Test the stability.",
        "Undervolt: mesma frequência com menos voltagem (menos calor e consumo). Teste a estabilidade.",
    ),
    ("voltage curve", "curva de voltagem"),
    ("negative = undervolt", "negativo = undervolt"),
    ("offset the voltage", "deslocar a voltagem"),
    ("memory max", "memória máx."),
    ("GPU max", "GPU máx."),
    ("Clock and voltage:", "Clock e voltagem:"),
    (
        "Writes the amdgpu driver option to /etc/modprobe.d and rebuilds the boot image. \
                         Works for any kernel, even after updates. Asks for the password; then just reboot.",
        "Grava a opção do driver amdgpu em /etc/modprobe.d e refaz a imagem de boot. \
                         Vale para qualquer kernel, inclusive depois de atualizar. Pede a senha; depois é só reiniciar.",
    ),
    ("Enable OverDrive", "Ativar OverDrive"),
    (
        "Clock/voltage (undervolt/overclock): the driver's OverDrive is off.",
        "Clock/voltagem (undervolt/overclock): o OverDrive do driver está desligado.",
    ),
    (
        "Clock/voltage: OverDrive turns on when you reboot the computer.",
        "Clock/voltagem: o OverDrive liga quando você reiniciar o computador.",
    ),
    (
        "AMD cards: the driver already tries to grow it at boot, even with the BIOS option off. If it could not, there was no room \
         in the PCI bridge window: the pci=realloc parameter (Machine → Kernel parameters) asks Linux to rearrange.",
        "Placas AMD: o driver já tenta aumentar sozinho no boot, mesmo com a opção desligada na BIOS. Se não conseguiu, faltou espaço \
         na janela da ponte PCI: o parâmetro pci=realloc (Máquina → Parâmetros do kernel) pede ao Linux para reorganizar.",
    ),
    (
        "On NVIDIA the driver only uses the large BAR if the BIOS enables Resizable BAR (and the card is an RTX 30 or newer).",
        "Na NVIDIA o driver só usa o BAR grande se a BIOS ligar o Resizable BAR (e a placa for RTX 30 ou mais nova).",
    ),
    (
        "First step, in the BIOS: enable \"Above 4G Decoding\" (sometimes called \"64-bit PCI\" or \"MMIO above 4G\"). It will not work without it.",
        "Primeiro passo, na BIOS: ligar \"Above 4G Decoding\" (às vezes chamado de \"64-bit PCI\" ou \"MMIO above 4G\"). Sem isso não dá.",
    ),
    (
        "no history yet (the service records every 30 s)",
        "sem histórico ainda (o serviço grava a cada 30 s)",
    ),
    ("memory", "memória"),
    (
        "no terminal found (konsole, gnome-terminal, kitty, alacritty, xterm)",
        "nenhum terminal encontrado (konsole, gnome-terminal, kitty, alacritty, xterm)",
    ),
    ("[finished with an error]", "[terminou com erro]"),
    ("[finished]", "[concluído]"),
    (
        "Tip: apps installed in the lab (apt install ... inside the Terminal) show up here after clicking \
                 \"Refresh apps\". To keep what you installed even after updating the image, prefer pip/conda inside ~/lab.",
        "Dica: apps instalados no laboratório (apt install ... dentro do Terminal) aparecem aqui ao clicar em \
                 \"Atualizar apps\". Para guardar o que instalou mesmo após atualizar a imagem, prefira pip/conda dentro de ~/lab.",
    ),
    (
        "System packages (installed by hand)",
        "Pacotes do sistema (instalados à mão)",
    ),
    ("Python packages", "Pacotes Python"),
    (
        "only apps from the lab and from distrobox containers (which have display access) can be opened",
        "só dá para abrir apps do laboratório e de containers distrobox (que têm acesso à tela)",
    ),
    ("no app with a shortcut", "nenhum app com atalho"),
    ("View apps", "Ver apps"),
    ("Refresh apps", "Atualizar apps"),
    ("Turn on", "Ligar"),
    ("Stop", "Parar"),
    ("lab", "laboratório"),
    ("No containers yet.", "Nenhum container ainda."),
    (
        "click \"View apps\" to list what is installed in each one",
        "clique em \"Ver apps\" para listar o que está instalado em cada um",
    ),
    ("~/lab folder", "Pasta ~/lab"),
    ("Opening PyCharm", "Abrindo o PyCharm"),
    (
        "Needs LAB_PYCHARM=1 in the image (Lab settings)",
        "Precisa de LAB_PYCHARM=1 na imagem (Configurações do laboratório)",
    ),
    (
        "Opening VS Code connected to the lab",
        "Abrindo o VS Code conectado ao laboratório",
    ),
    (
        "Needs the Remote - SSH extension",
        "Precisa da extensão Remote - SSH",
    ),
    ("VS Code (PC app)", "VS Code (app do PC)"),
    (
        "VS Code password copied (paste it on the login screen)",
        "Senha do VS Code copiada (cole na tela de login)",
    ),
    ("VS Code (browser)", "VS Code (navegador)"),
    ("Open:", "Abrir:"),
    (
        "Rolling back to the previous image",
        "Voltando para a imagem anterior",
    ),
    ("Roll back version", "Voltar versão"),
    ("Rebuilding the lab", "Reconstruindo o laboratório"),
    ("Update Python packages", "Atualizar pacotes Python"),
    ("Updating the lab", "Atualizando o laboratório"),
    (
        "New PyTorch/ROCm base; tests on the GPUs and reverts by itself if it fails",
        "Base nova do PyTorch/ROCm; testa nas GPUs e volta sozinho se falhar",
    ),
    ("GPU test in the lab", "Teste das GPUs no laboratório"),
    ("Test GPUs", "Testar GPUs"),
    ("Turning the lab on", "Ligando o laboratório"),
    (
        "Create and turn on (slow the 1st time)",
        "Criar e ligar (demora na 1ª vez)",
    ),
    ("Turning the lab off", "Desligando o laboratório"),
    ("Turn off", "Desligar"),
    (
        "PyTorch on the MI50s + pandas, scikit-learn, transformers... Your files stay in ~/lab.",
        "PyTorch nas MI50 + pandas, scikit-learn, transformers... Seus arquivos ficam em ~/lab.",
    ),
    ("not created yet", "ainda não criado"),
    ("Data and AI lab", "Laboratório de dados e IA"),
    (
        "Install podman:  sudo pacman -S podman",
        "Instale o podman:  sudo pacman -S podman",
    ),
    ("no podman/docker", "sem podman/docker"),
    (
        "VS Code is not installed on the PC",
        "VS Code não está instalado no PC",
    ),
    (
        "AI server settings saved",
        "Ajustes do servidor de IA salvos",
    ),
    ("Open chat", "Abrir chat"),
    ("AI server log", "Log do servidor de IA"),
    ("View log", "Ver log"),
    (
        "Tests 1 or all GPUs, layer or row split, flash attention and q8_0 cache with the chosen model and saves the fastest combination. With the server stopped; takes a few minutes.",
        "Testa 1 ou todas as GPUs, divisão por camadas ou linhas, flash attention e cache q8_0 com o modelo escolhido e grava a combinação mais rápida. Com o servidor parado; leva alguns minutos.",
    ),
    ("Optimize (tokens/s)", "Otimizar (tokens/s)"),
    ("Updating llama.cpp", "Atualizando o llama.cpp"),
    ("Update llama.cpp", "Atualizar llama.cpp"),
    ("Save settings", "Salvar ajustes"),
    ("Restarting the AI server", "Reiniciando o servidor de IA"),
    ("Restart with these settings", "Reiniciar com estes ajustes"),
    ("Stopping the AI server", "Parando o servidor de IA"),
    ("Starting the AI server", "Iniciando o servidor de IA"),
    ("Start", "Iniciar"),
    (
        "e.g. --flash-attn on -ctk q8_0 -ctv q8_0",
        "ex.: --flash-attn on -ctk q8_0 -ctv q8_0",
    ),
    ("Extra arguments", "Argumentos extras"),
    ("Port", "Porta"),
    ("Context (tokens)", "Contexto (tokens)"),
    ("Both MI50s (split model)", "As duas MI50 (modelo dividido)"),
    ("Model", "Modelo"),
    ("Models folder", "Pasta dos modelos"),
    (
        "Runs .gguf models on the MI50s with an OpenAI-compatible API (works with Open WebUI, \
             Continue, scripts...). Each GPU uses the processors and memory of its own socket.",
        "Roda modelos .gguf nas MI50 com uma API compatível com a da OpenAI (serve para Open WebUI, \
             Continue, scripts...). Cada GPU usa os processadores e a memória do próprio socket.",
    ),
    ("loading the model...", "carregando o modelo..."),
    ("AI server (llama.cpp)", "Servidor de IA (llama.cpp)"),
    ("Reload", "Recarregar"),
    ("Kernel parameters", "Parâmetros do kernel"),
    (
        "Asks for the password, backs up the bootloader file and rewrites the entries. Applies after a reboot.",
        "Pede a senha, faz backup do arquivo do bootloader e regrava as entradas. Vale depois de reiniciar.",
    ),
    ("Apply at boot", "Aplicar no boot"),
    ("e.g. amd_iommu=on", "ex.: amd_iommu=on"),
    ("Other:", "Outro:"),
    (
        "Bootloader not recognized (Limine, systemd-boot/sdboot-manage, GRUB or rEFInd): cannot be edited from here.",
        "Bootloader não reconhecido (Limine, systemd-boot/sdboot-manage, GRUB ou rEFInd): não dá para editar daqui.",
    ),
    (
        "The kernel reported no vulnerabilities (or this processor is not affected).",
        "O kernel não informou vulnerabilidades (ou este processador não é afetado).",
    ),
    ("Gain", "Ganho"),
    ("Risk without it", "Risco sem ela"),
    ("Status on your processor", "Situação no seu processador"),
    (
        "Turn all off (mitigations=off)",
        "Desligar todas (mitigations=off)",
    ),
    ("All on (default)", "Todas ligadas (padrão)"),
    ("Turn all off", "Desligar todas"),
    (
        "Turn off the light ones (keeps Spectre and Meltdown)",
        "Desligar as leves (mantém Spectre e Meltdown)",
    ),
    (
        "Protections against side-channel attacks. Turning them off gives performance back, but lets malicious programs read \
                 data from other programs or from the kernel. They apply after a reboot.",
        "Proteções contra ataques de canal lateral. Desligar devolve desempenho, mas deixa programas maliciosos lerem \
                 dados de outros programas ou do kernel. Valem depois de reiniciar.",
    ),
    ("Processor mitigations", "Mitigações do processador"),
    ("Through IPMI (read-only):", "Pelo IPMI (só leitura):"),
    ("the hottest of all", "o mais quente de tudo"),
    ("Hottest GPU", "GPU mais quente"),
    ("Hottest CPU", "CPU mais quente"),
    ("Follow:", "Seguir:"),
    ("board's own", "da placa"),
    ("name (e.g. front, CPU 0)", "nome (ex.: frente, CPU 0)"),
    (
        "No controllable fan found. Regular boards need the sensor chip's driver \
                     (e.g. sudo modprobe nct6775; sensors-detect tells which); server boards use IPMI (sudo pacman -S ipmitool).",
        "Nenhuma ventoinha controlável encontrada. Placas comuns precisam do driver do chip de sensores \
                     (ex.: sudo modprobe nct6775; o sensors-detect diz qual); placas de servidor usam o IPMI (sudo pacman -S ipmitool).",
    ),
    ("Motherboard fans", "Ventoinhas da placa-mãe"),
    (
        "The service applies them; if it stops, everything goes back to what it was.",
        "Quem aplica é o serviço; se ele parar, tudo volta ao que era.",
    ),
    ("Apply machine settings", "Aplicar ajustes da máquina"),
    (
        "Fans through the BMC (IPMI):",
        "Ventoinhas pelo BMC (IPMI):",
    ),
    (
        "scx_loader not installed (sudo pacman -S scx-scheds scx-tools).",
        "scx_loader não instalado (sudo pacman -S scx-scheds scx-tools).",
    ),
    ("kernel default", "padrão do kernel"),
    (
        "kernel default (turn sched_ext off)",
        "padrão do kernel (desligar sched_ext)",
    ),
    ("Scheduler (sched_ext)", "Escalonador (sched_ext)"),
    (
        "2 MB pages set aside at boot, for programs that use hugetlbfs (virtual machines, DPDK, some databases). \
                     llama.cpp and PyTorch do not use them: for those, THP and memory on the right socket are what count.",
        "Páginas de 2 MB separadas no boot, para programas que usam hugetlbfs (máquinas virtuais, DPDK, alguns bancos de dados). \
                     O llama.cpp e o PyTorch não usam: para eles valem THP e a memória no socket certo.",
    ),
    ("Reserved hugepages", "Hugepages reservadas"),
    (
        "always: fewer TLB misses (good for AI and CPU rendering); madvise: only for those that ask (default).",
        "always: menos falhas de TLB (bom para IA e render na CPU); madvise: só quem pede (padrão).",
    ),
    (
        "On: prefers freeing cache on the same node over using memory from the other. Good for latency, bad for disk cache.",
        "Ligado: prefere liberar cache do mesmo nó a usar memória do outro. Bom para latência, ruim para cache de disco.",
    ),
    (
        "Reclaim memory on the same node (zone_reclaim)",
        "Recuperar memória no próprio nó (zone_reclaim)",
    ),
    (
        "The kernel moves pages by itself closer to whoever uses them. With numapilot placing the programs, turning it off avoids migrations that cause micro-stutter.",
        "O kernel move páginas sozinho para perto de quem usa. Com o numapilot cuidando do lugar dos programas, desligar evita migrações que causam travadinhas.",
    ),
    ("Kernel NUMA balancing", "Balanceamento NUMA do kernel"),
    ("uncore at maximum", "uncore no máximo"),
    ("Allowed states:", "Estados permitidos:"),
    (
        "Applies while a program with \"Low latency\" runs (the Game profile turns it on): on its cores, the processor does not enter \
                 the deep idle states (waking from them causes micro-stutter) and the socket's uncore stays at maximum. On close, everything goes back.",
        "Vale enquanto roda um programa com \"Baixa latência\" (o perfil Jogo liga): nos núcleos dele, o processador não entra \
                 nos estados de economia profundos (acordar deles causa travadinhas) e o uncore do socket fica no máximo. Ao fechar, volta tudo.",
    ),
    ("Gaming latency", "Latência em jogos"),
    (
        "How much the processor may draw for a few seconds before dropping to the limit above.",
        "Quanto o processador pode puxar por alguns segundos antes de cair para o limite acima.",
    ),
    ("Power peak (PL2)", "Pico de potência (PL2)"),
    ("unavailable (RAPL)", "indisponível (RAPL)"),
    (
        "unavailable (sudo modprobe intel_uncore_frequency)",
        "indisponível (sudo modprobe intel_uncore_frequency)",
    ),
    (
        "Higher uncore = lower memory and L3 cache latency, at the cost of a few watts.",
        "Uncore mais alto = menor latência de memória e do cache L3, à custa de alguns watts.",
    ),
    ("Uncore (L3 + memory)", "Uncore (L3 + memória)"),
    ("Min / max frequency", "Frequência mín. / máx."),
    (
        "Ryzen's main knob (amd-pstate): from performance to power. With the performance governor the driver locks it to performance.",
        "O ajuste principal do Ryzen (amd-pstate): de performance a power. Com o governor performance o driver fixa em performance.",
    ),
    ("Energy preference (EPP)", "Preferência de energia (EPP)"),
    ("Energy bias (EPB)", "Viés de energia (EPB)"),
    ("Now", "Agora"),
    ("Processor", "Processador"),
    ("this adapter does not support it", "esta placa não suporta"),
    ("now: ? (install ethtool)", "agora: ? (instale o ethtool)"),
    ("now: off", "agora: desligado"),
    ("now: on", "agora: ligado"),
    (
        "Lets this PC be turned on from another one's panel (My PCs → Wake). Also needs the BIOS option (Wake on LAN / Power on by PCI-E) and the network cable.",
        "Deixa este PC ser ligado pelo painel de outro (Meus PCs → Ligar). Precisa também da opção na BIOS (Wake on LAN / Power on by PCI-E) e do cabo de rede.",
    ),
    (
        "Wake over the network (Wake-on-LAN)",
        "Ligar pela rede (Wake-on-LAN)",
    ),
    ("ERRORS", "ERROS"),
    ("SMART FAILED", "SMART FALHOU"),
    (
        "To see the disks: sudo pacman -S smartmontools",
        "Para ver os discos: sudo pacman -S smartmontools",
    ),
    ("No problems found.", "Nenhum problema encontrado."),
    (
        "The service collects 1 min after starting and then every 30 min.",
        "O serviço coleta 1 min depois de iniciar e depois a cada 30 min.",
    ),
    ("last 14 days", "últimos 14 dias"),
    (
        "The service measures processors and graphics cards. Motherboard, memory, disks, fans and power supply losses add some 30-80 W; enter it so the figure gets close to the wall (or measure with a wall meter).",
        "O serviço mede processadores e placas de vídeo. Placa-mãe, memória, discos, ventoinhas e a perda da fonte somam uns 30-80 W; informe para a conta ficar perto da tomada (ou meça com um medidor de tomada).",
    ),
    ("Rest of the PC (W):", "Resto do PC (W):"),
    ("kWh price (R$):", "Preço do kWh (R$):"),
    ("Energy", "Energia"),
    ("+ schedule", "+ horário"),
    ("invalid time (use HH:MM)", "horário inválido (use HH:MM)"),
    ("to", "às"),
    (
        "E.g. Quiet from 23:00 to 07:00. A program with \"machine mode\" in its profile (Adjust…) takes priority; switching the mode by hand holds until the rule changes.",
        "Ex.: Silencioso das 23:00 às 07:00. Programa com \"modo da máquina\" no perfil (Ajustar…) tem prioridade; trocar o modo à mão vale até a regra mudar.",
    ),
    ("Scheduled modes", "Modos por horário"),
    ("RGB keyboard", "Teclado RGB"),
    ("maximum", "máximo"),
    ("automatic", "automático"),
    (
        "Fans, the 80% charge limit and RGB need the community module Linuwu-Sense.",
        "Ventoinhas, limite de carga em 80% e RGB precisam do módulo da comunidade Linuwu-Sense.",
    ),
    (
        "Lowering STAPM and the temperature keeps the laptop cool and quiet; the service reapplies if the firmware resets the values when switching AC/battery.",
        "Baixar o STAPM e a temperatura deixa o notebook frio e silencioso; o serviço reaplica se o firmware voltar os valores ao trocar tomada/bateria.",
    ),
    ("now ? (service)", "agora ? (serviço)"),
    ("Maximum temperature", "Temperatura máxima"),
    ("Peaks (fast PPT)", "Picos (PPT rápido)"),
    ("Short average (slow PPT)", "Média curta (PPT lento)"),
    ("Sustained (STAPM)", "Sustentado (STAPM)"),
    ("install (AUR)", "instalar (AUR)"),
    (
        "Install ryzenadj to tune STAPM/PPT (the equivalent of Intel's PL1/PL2).",
        "Instale o ryzenadj para ajustar STAPM/PPT (o equivalente do PL1/PL2 da Intel).",
    ),
    ("Ryzen power limits", "Limites de potência do Ryzen"),
    (
        "Shifts power from the processor to the graphics card in games (nvidia-powerd).",
        "Passa potência do processador para a placa de vídeo em jogos (nvidia-powerd).",
    ),
    ("NVIDIA Dynamic Boost", "Dynamic Boost da NVIDIA"),
    (
        "These programs opened the dedicated card; while they are open it does not sleep and drains the battery. Open them on the integrated GPU (program profile → card) to save power.",
        "Esses programas abriram a placa dedicada; enquanto estiverem abertos ela não dorme e gasta bateria. Abra-os na integrada (perfil do programa → placa) para economizar.",
    ),
    ("always on", "sempre ligada"),
    ("sleep when nobody uses it", "dormir quando ninguém usa"),
    (
        "Vendor profile (ACPI): changes fans and power limits. The modes already pick one.",
        "Perfil do fabricante (ACPI): mexe em ventoinhas e limites de potência. Os modos já escolhem um.",
    ),
    ("Laptop power profile", "Perfil de energia do notebook"),
    (
        "Keeping the battery at 80% when the laptop is always plugged in greatly extends its lifespan.",
        "Deixar a bateria em 80% quando o notebook fica sempre na tomada aumenta muito a vida útil dela.",
    ),
    ("Stop charging at", "Parar de carregar em"),
    (" (now)", " (agora)"),
    (
        "Xwayland on the integrated GPU only",
        "Xwayland só na integrada",
    ),
    (
        "Installs /usr/local/bin/Xwayland that uses only Mesa (asks for the password). X11 games set to the NVIDIA still go to it. Applies at the next login.",
        "Instala /usr/local/bin/Xwayland que usa só o Mesa (pede a senha). Jogos X11 escolhidos para a NVIDIA continuam indo para ela. Vale no próximo login.",
    ),
    (
        "use only the integrated GPU in Xwayland",
        "usar só a integrada no Xwayland",
    ),
    (
        "Xwayland may hold the NVIDIA for no reason.",
        "O Xwayland pode prender a NVIDIA à toa.",
    ),
    (
        "Startup back to normal (next login)",
        "Inicialização de volta ao normal (próximo login)",
    ),
    ("nothing new", "nada novo"),
    (
        "Startup programs (ZapZap, notifiers...) and user services (e.g. Sunshine) \
                             use only the 680M, so they do not wake the NVIDIA. Game launchers (Steam, Heroic, Lutris) \
                             stay as they are: games keep running on the NVIDIA. Applies from the next login.",
        "Os programas da inicialização (ZapZap, notificadores...) e os serviços do usuário (ex.: Sunshine) \
                             passam a usar só a 680M, então não acordam a NVIDIA. Lançadores de jogos (Steam, Heroic, Lutris) \
                             ficam como estão: os jogos continuam na NVIDIA. Vale a partir do próximo login.",
    ),
    (
        "Everything that starts with the system on the integrated GPU",
        "Tudo que abre com o sistema na integrada",
    ),
    (
        "leave as is (sleeps when nobody uses it)",
        "não mexer (dorme quando ninguém usa)",
    ),
    (
        "Switches right away, without leaving the session: \"off\" removes the card from the system (zero power, nothing wakes it, KDE carries on \
                         with the integrated GPU only and HDMI does not work); \"available\" brings it back and KDE adds it by itself. \
                         It only turns off if no program is using the NVIDIA.",
        "Troca na hora, sem sair da sessão: \"desligada\" tira a placa do sistema (zero consumo, nada a acorda, o KDE segue \
                         só na integrada e o HDMI não funciona); \"disponível\" devolve e o KDE a adiciona sozinho. \
                         Só desliga se nenhum programa estiver usando a NVIDIA.",
    ),
    ("no display", "nenhuma tela"),
    ("holding at the limit", "parada no limite"),
    (
        "No turbo, power-saving governor, EPB power saving, power limited to 60% and LAVD in power-saving mode.",
        "Sem turbo, governor econômico, EPB economia, potência limitada a 60% e LAVD em economia.",
    ),
    (
        "Both sockets in performance, uncore at maximum, THP always and no automatic NUMA balancing.",
        "Os dois sockets em desempenho, uncore no máximo, THP sempre e sem balanceamento NUMA automático.",
    ),
    (
        "Graphics card's socket at maximum performance, uncore at maximum, turbo, no automatic NUMA balancing and the LAVD scheduler in gaming mode.",
        "Socket da placa de vídeo em desempenho máximo, uncore no máximo, turbo, sem balanceamento NUMA automático e escalonador LAVD em modo jogo.",
    ),
    (
        "Everything as the system was.",
        "Tudo como o sistema estava.",
    ),
    (
        "One click tunes processors, memory and scheduler together. \"Normal\" puts everything back as the system was.",
        "Um clique ajusta processadores, memória e escalonador juntos. \"Normal\" devolve tudo como o sistema estava.",
    ),
    ("balanced (power saving)", "equilibrado (economia)"),
    ("balanced (performance)", "equilibrado (desempenho)"),
    ("Back to automatic", "Voltar ao automático"),
    ("AI / ROCm", "IA / ROCm"),
    ("game / graphics", "jogo / gráfico"),
    ("GPU usage", "Uso da GPU"),
    ("Program", "Programa"),
    (
        "No saved choices: everything on automatic.",
        "Nenhuma escolha salva: tudo no automático.",
    ),
    ("Import choices", "Importar escolhas"),
    (
        "Brings in the choices from the file. Cores and cards from another PC are adapted (they go back to automatic).",
        "Traz as escolhas do arquivo. Núcleos e placas de outro PC são adaptados (voltam para o automático).",
    ),
    ("Import", "Importar"),
    (
        "exported .json file (full path)",
        "arquivo .json exportado (caminho completo)",
    ),
    (
        "Writes all choices to a file, to take to another PC or share.",
        "Grava todas as escolhas num arquivo, para levar a outro PC ou compartilhar.",
    ),
    ("Export…", "Exportar…"),
    (
        "Programs with their own choices. The ones not listed here stay on automatic.",
        "Programas com escolhas próprias. Os que não estão aqui ficam no automático.",
    ),
    (
        "Menu shortcut restored to the original",
        "Atalho do menu voltou ao original",
    ),
    (
        "Copied! Paste into Steam → game Properties → Launch options.",
        "Copiado! Cole em Steam → Propriedades do jogo → Opções de inicialização.",
    ),
    (
        "Makes the menu shortcut open this program through numapilot, so the chosen GPU always applies.",
        "Faz o atalho do menu abrir este programa pelo numapilot, para a GPU escolhida valer sempre.",
    ),
    (
        "The menu shortcut already opens through numapilot. Click to undo.",
        "O atalho do menu já abre pelo numapilot. Clique para desfazer.",
    ),
    ("Use in the menu", "Usar no menu"),
    ("In the menu: yes", "No menu: sim"),
    (
        "Makes this program open when you log in (the Startup tab has everything)",
        "Faz este programa abrir quando você entra na sessão (aba Inicialização tem tudo)",
    ),
    (
        "Already opens when you log in. Click to stop that (the Startup tab has everything).",
        "Já abre quando você entra na sessão. Clique para não abrir mais (aba Inicialização tem tudo).",
    ),
    ("Open at startup", "Abrir ao iniciar"),
    ("Opens at startup: yes", "Abre ao iniciar: sim"),
    (
        "Closes and opens it again with the chosen CPU and GPU",
        "Fecha e abre de novo com a CPU e a GPU escolhidas",
    ),
    ("Reopen", "Reabrir"),
    ("Copy Steam option", "Copiar opção Steam"),
    (
        "Cores, memory, priority, GPU and ready-made profiles",
        "Núcleos, memória, prioridade, GPU e perfis prontos",
    ),
    (
        "reopen the program to switch the GPU",
        "reabra o programa para trocar a GPU",
    ),
    (
        "Same GPU as the game (official lsfg-vk)",
        "Mesma GPU do jogo (lsfg-vk oficial)",
    ),
    ("Lossless Scaling: same GPU", "Lossless Scaling: mesma GPU"),
    (
        "Part of the memory is on the other socket: every access to it is slower.",
        "Parte da memória está no outro socket: cada acesso a ela é mais lento.",
    ),
    ("no GPU", "sem GPU"),
    ("both sockets", "os dois sockets"),
    ("free (all)", "livre (todos)"),
    (
        "Memory in use by the program (adding up its processes)",
        "Memória em uso pelo programa (somando os processos dele)",
    ),
    (
        "Memory in use / limit chosen in Adjust…",
        "Memória em uso / limite escolhido em Ajustar…",
    ),
    (
        "you can close this window; the command keeps running in the background",
        "pode fechar esta janela; o comando continua em segundo plano",
    ),
    ("failed", "com erro"),
    ("finished", "concluído"),
    ("running…", "rodando…"),
    (
        "Wait for the previous command to finish",
        "Espere o comando anterior terminar",
    ),
    ("No GPU", "Nenhuma GPU"),
    (", own monitor", ", monitor próprio"),
    ("Both sockets", "Os dois sockets"),
    ("Socket of the chosen GPU", "Socket da GPU escolhida"),
    (
        "Charts: last minute, on the same scale (solid line = read/download).",
        "Gráficos: último minuto, na mesma escala (linha cheia = leitura/download).",
    ),
    ("Network", "Rede"),
    ("Measuring...", "Medindo..."),
    ("Disks and network", "Discos e rede"),
    ("Diagnosis", "Diagnóstico"),
    (
        "Full diagnosis (gfx906-rocm doctor)",
        "Diagnóstico completo (gfx906-rocm doctor)",
    ),
    ("CPUs \\ memory", "CPUs \\ memória"),
    (
        "Not measured yet. Click \"Measure now\".",
        "Ainda não medido. Clique em \"Medir agora\".",
    ),
    (
        "Measuring memory latency and bandwidth",
        "Medindo latência e banda de memória",
    ),
    (
        "Takes ~10 s; the PC is busy during the test",
        "Leva ~10 s; o PC fica ocupado durante o teste",
    ),
    ("Measure now", "Medir agora"),
    (
        "Latency and bandwidth between sockets (measured)",
        "Latência e banda entre sockets (medidas)",
    ),
    ("Not measured yet.", "Ainda não medido."),
    (
        "Too small to use as a criterion: the system order stays.",
        "Pequena demais para usar como critério: a ordem do sistema continua.",
    ),
    (
        "In use: programs with a chosen number of cores get the fastest ones.",
        "Em uso: os programas com número de núcleos escolhido ficam nos mais rápidos.",
    ),
    (
        "Measuring cache latency per core",
        "Medindo a latência do cache por núcleo",
    ),
    (
        "Measures L3 cache access from each core (a few seconds, with the PC idle). If the difference is larger than the noise, core choices, including maximum turbo, start preferring the fastest ones.",
        "Mede o acesso ao cache L3 a partir de cada núcleo (alguns segundos, com o PC parado). Se a diferença for maior que o ruído, as escolhas de núcleos, inclusive o turbo máximo, passam a preferir os mais rápidos.",
    ),
    ("Measure cores", "Medir núcleos"),
    ("Cache latency per core", "Latência do cache por núcleo"),
    (
        "irqbalance is running and may undo the GPU interrupt tuning: \
                     sudo systemctl disable --now irqbalance",
        "O irqbalance está rodando e pode desfazer o ajuste das interrupções das GPUs: \
                     sudo systemctl disable --now irqbalance",
    ),
    (
        "GPU interrupts: handled by each card's socket.",
        "Interrupções das GPUs: atendidas pelo socket de cada placa.",
    ),
    ("unavailable", "indisponível"),
    (
        "performance = always maximum frequency (games); schedutil/powersave = saves power.",
        "performance = frequência máxima sempre (jogos); schedutil/powersave = economiza energia.",
    ),
    ("Governor", "Governador"),
    (
        "Memory pages that had to be created on the other socket (numastat other_node/numa_miss), per second.",
        "Páginas de memória que precisaram ser criadas no outro socket (numastat other_node/numa_miss), por segundo.",
    ),
    (
        "Allocations on the other socket",
        "Alocações no outro socket",
    ),
    ("Memory in use", "Memória em uso"),
    (
        "Memory and frequency per socket",
        "Memória e frequência por socket",
    ),
    (
        "Each square is a physical core (split into its 2 threads). Color = usage; white dot = some program \
                     is pinned to it (hover to see).",
        "Cada quadrado é um núcleo físico (dividido nos 2 threads). Cor = uso; ponto branco = algum programa \
                     está preso a ele (passe o mouse para ver).",
    ),
    ("Core map", "Mapa de núcleos"),
    (
        "Hover over a chart to see the core and the frequency; yellow dot = some program is pinned to it.",
        "Passe o mouse num gráfico para ver o núcleo e a frequência; ponto amarelo = algum programa está preso a ele.",
    ),
    (
        "Processor temperature (last minute, 30–100 °C)",
        "Temperatura do processador (último minuto, 30–100 °C)",
    ),
    (
        "The deep idle states (C3/C6) are off: every core counts as \
                         awake and turbo stays at the all-core value. Turn them on in Machine → Processor.",
        "Os estados de economia profundos (C3/C6) estão desligados: todos os núcleos contam como \
                         acordados e o turbo fica no valor de todos os núcleos. Ligue-os em Máquina → Processador.",
    ),
    (
        "The turbo table comes from the numapilot service (Intel processors only).",
        "A tabela de turbo vem do serviço do numapilot (só processadores Intel).",
    ),
    (
        "Makes the menu shortcut open this program through numapilot, so the graphics card choice applies. Can be undone in Saved choices.",
        "Faz o atalho do menu abrir este programa pelo numapilot, para a escolha de placa de vídeo valer. Dá para desfazer em Escolhas salvas.",
    ),
    ("Adjust the menu", "Ajustar o menu"),
    (
        "Opened from the menu, this program already goes through numapilot (the card choice applies).",
        "Aberto pelo menu, este programa já passa pelo numapilot (a escolha de placa vale).",
    ),
    ("menu adjusted", "menu ajustado"),
    ("open now", "aberto agora"),
    ("saved choice", "escolha salva"),
    (
        "No program found with this search.",
        "Nenhum programa encontrado com essa busca.",
    ),
    (
        "The choice is saved and applies when the program starts. Processor and memory apply on any \
                     start; for the graphics card, open it from the adjusted menu (option in Adjust… itself) or with Reopen.",
        "A escolha fica salva e vale quando o programa abrir. Processador e memória valem em qualquer \
                     abertura; para a placa de vídeo, abra pelo menu ajustado (opção no próprio Ajustar…) ou pelo Reabrir.",
    ),
    (
        "🔍 Program name, executable or source",
        "🔍 Nome do programa, executável ou origem",
    ),
    ("Ending", "Encerrando"),
    ("Forced stop:", "Parada forçada:"),
    (
        "Anything unsaved in it is lost.",
        "O que não foi salvo nele se perde.",
    ),
    (
        "cannot be ended from here",
        "não podem ser encerrados por aqui",
    ),
    (
        "No program in use right now. Open something (a game, Blender, llama-server...) and it shows up here.",
        "Nenhum programa sendo usado agora. Abra algo (um jogo, o Blender, o llama-server...) e ele aparece aqui.",
    ),
    (
        "Click a row to select it; the arrow (or double-click) shows the processes and lets you choose processor and card. CPU changes right away; the GPU applies when the program starts.",
        "Clique numa linha para selecionar; na seta (ou dois cliques) para ver os processos e escolher processador e placa. CPU muda na hora; a GPU vale quando o programa abre.",
    ),
    (
        "Lists the installed programs (menu, Flatpak, Snap, AppImage, Steam games) to choose processor, memory and card without having to open the program first.",
        "Lista os programas instalados (menu, Flatpak, Snap, AppImage, jogos da Steam) para escolher processador, memória e placa sem precisar abrir o programa antes.",
    ),
    ("🔍 Search", "🔍 Buscar"),
    ("Steam: games", "Steam: jogos"),
    ("View list", "Ver lista"),
    (
        "Steam: remove from all games",
        "Steam: remover de todos os jogos",
    ),
    ("Remove from all", "Remover de todos"),
    (
        "Steam: apply to all games",
        "Steam: aplicar em todos os jogos",
    ),
    (
        "Puts \"numapilot run -- %command%\" in the launch options of every installed game (keeps what was already there). Close Steam first.",
        "Coloca \"numapilot run -- %command%\" nas opções de inicialização de todos os jogos instalados (mantém o que já existia). Feche a Steam antes.",
    ),
    ("Apply to all games", "Aplicar em todos os jogos"),
    (
        "Asks the program to close (SIGTERM), like closing the window",
        "Pede para o programa fechar (SIGTERM), como fechar a janela",
    ),
    ("■ End", "■ Encerrar"),
    (
        "Closes it right away (SIGKILL). Anything unsaved is lost.",
        "Fecha na hora (SIGKILL). O que não foi salvo se perde.",
    ),
    ("⊗ Force stop", "⊗ Forçar parada"),
    (
        "Select a program in the list",
        "Selecione um programa na lista",
    ),
    ("last 200 lines", "últimas 200 linhas"),
    ("Copy", "Copiar"),
    ("Restart", "Reiniciar"),
    ("At boot", "No boot"),
    ("State", "Estado"),
    ("Service", "Serviço"),
    ("No service in this filter.", "Nenhum serviço nesse filtro."),
    (
        "To choose what starts at boot, use the Startup tab.",
        "Para escolher o que abre no boot, use a aba Inicialização.",
    ),
    ("All", "Todos"),
    ("Stopped", "Parados"),
    ("Running", "Rodando"),
    ("User", "Do usuário"),
    (
        "System: they run from boot. Start, stop and restart ask for the password.",
        "Do sistema: rodam desde o boot. Iniciar, parar e reiniciar pedem a senha.",
    ),
    (
        "Your user's: they run from login, with no password.",
        "Do seu usuário: rodam a partir do login, sem pedir senha.",
    ),
    (
        "UEFI, TPM, cores and memory from the card's socket; then install Windows through virt-manager",
        "UEFI, TPM, núcleos e memória do socket da placa; depois instale o Windows pelo virt-manager",
    ),
    ("Create the VM", "Criar a VM"),
    (" cores", " núcleos"),
    ("Memory / cores / disk", "Memória / núcleos / disco"),
    ("Windows ISO", "ISO do Windows"),
    ("Open virt-manager", "Abrir virt-manager"),
    ("Giving the card back", "Devolvendo a placa"),
    ("Give back to Linux", "Devolver ao Linux"),
    ("Preparing the VM", "Preparando a VM"),
    (
        "Binds the card to vfio-pci at boot, enables the IOMMU and rebuilds the boot image (asks for the password). Reboot afterwards.",
        "Prende a placa no vfio-pci no boot, liga o IOMMU e refaz a imagem de boot (pede a senha). Reinicie depois.",
    ),
    ("Prepare the card", "Preparar a placa"),
    ("Check", "Conferir"),
    ("VM card:", "Placa da VM:"),
    (
        "One graphics card is reserved for Windows in a VM (VFIO). Linux stays on the other. Some anti-cheats (e.g. Vanguard) refuse VMs: check the game first.",
        "Uma placa de vídeo fica só para o Windows numa VM (VFIO). O Linux continua na outra. Alguns anti-cheats (ex.: Vanguard) recusam VM: confira o jogo antes.",
    ),
    (
        "Windows virtual machine (games with anti-cheat)",
        "Máquina virtual Windows (jogos com anti-cheat)",
    ),
    ("Performance", "Desempenho"),
    ("Result", "Resultado"),
    ("Version", "Versão"),
    ("What", "O quê"),
    ("Date", "Data"),
    ("Update history", "Histórico das atualizações"),
    ("Community news", "Novidades da comunidade"),
    (
        "Validating the current versions",
        "Validando as versões atuais",
    ),
    (
        "Runs the tests on the GPUs and saves the current performance as the reference",
        "Roda os testes nas GPUs e grava o desempenho de agora como referência",
    ),
    ("Validate the current versions", "Validar as versões atuais"),
    ("Updating with validation", "Atualizando com validação"),
    (
        "Update now (with validation)",
        "Atualizar agora (com validação)",
    ),
    ("Looking for new versions", "Procurando versões novas"),
    ("Check now", "Verificar agora"),
    ("Not checked yet.", "Ainda não verificado."),
    (
        "Keeps the current version even with automatic updates",
        "Mantém a versão atual mesmo com atualização automática",
    ),
    ("Pin", "Travar"),
    ("Available", "Disponível"),
    ("In use", "Em uso"),
    ("Native ROCm", "ROCm nativo"),
    ("ROCm image", "Imagem ROCm"),
    ("Lab (PyTorch)", "Laboratório (PyTorch)"),
    (
        "Vulkan (GPUs without ROCm: the lab uses the CPU and the AI server uses the GPU through Vulkan)",
        "Vulkan (GPUs sem ROCm: o laboratório usa a CPU e o servidor de IA usa a GPU pelo Vulkan)",
    ),
    ("AMD's official ROCm", "ROCm oficial da AMD"),
    ("MI50 with community ROCm", "MI50 com ROCm da comunidade"),
    (
        "Update preference saved",
        "Preferência de atualização salva",
    ),
    ("just notify me", "só me avisar"),
    (
        "install if it passes validation",
        "instalar se passar na validação",
    ),
    ("When there is a new version:", "Quando houver versão nova:"),
    ("No local network detected.", "Sem rede local detectada."),
    (
        "The link contains the access password; do not share it. Away from home, use a VPN (e.g. Tailscale).",
        "O link tem a senha de acesso; não compartilhe. Fora de casa, use uma VPN (ex.: Tailscale).",
    ),
    ("Copy link", "Copiar link"),
    (
        "Point the phone camera or open:",
        "Aponte a câmera do celular ou abra:",
    ),
    ("Phone panel", "Painel no celular"),
    (
        "Turn on the panel (starts with the session)",
        "Ligar o painel (inicia junto com a sessão)",
    ),
    (
        "See temperatures and usage, switch a program's profile and turn on the lab/AI server from the phone (same network).",
        "Veja temperaturas e uso, troque o perfil de um programa e ligue o laboratório/servidor de IA pelo celular (mesma rede).",
    ),
    (
        "Downloads the new version, rebuilds and reinstalls in a terminal (asks for the password). Then reopen the interface.",
        "Baixa a versão nova, recompila e reinstala num terminal (pede a senha). Depois, reabra a interface.",
    ),
    ("Update numapilot", "Atualizar o numapilot"),
    ("unknown source", "origem desconhecida"),
    ("Add", "Adicionar"),
    ("name (e.g. BC-250)", "nome (ex.: BC-250)"),
    ("PC backup", "Backup dos PCs"),
    (
        "Stores the settings of each PC that is on in ~/numapilot-backups (the phone panel does this by itself once a day)",
        "Guarda as configurações de cada PC ligado em ~/numapilot-backups (o painel no celular faz isso sozinho uma vez por dia)",
    ),
    ("Back up all now", "Backup de todos agora"),
    (
        "Brings in the program profiles (Adjust…) this PC does not have yet",
        "Traz os perfis de programa (Ajustar…) que este PC ainda não tem",
    ),
    ("copy profiles", "copiar perfis"),
    (
        "Sends the Wake-on-LAN packet",
        "Manda o pacote do Wake-on-LAN",
    ),
    (
        "Paste the link of another PC's panel (Settings → Phone panel, on that PC): this one's panel shows them all together.",
        "Cole o link do painel de outro PC (Configurações → Painel no celular, lá nele): o painel deste mostra todos juntos.",
    ),
    ("My other PCs", "Meus outros PCs"),
    ("open in the browser", "abrir no navegador"),
    ("Test notification sent", "Aviso de teste enviado"),
    (
        "If this arrived, notifications are working.",
        "Se chegou, os avisos estão funcionando.",
    ),
    ("numapilot test", "Teste do numapilot"),
    ("Send test", "Mandar teste"),
    ("Save", "Salvar"),
    ("finished tasks", "tarefas prontas"),
    ("other PCs", "outros PCs"),
    ("disks/memory/battery", "discos/memória/bateria"),
    ("Hot GPU", "GPU quente"),
    ("Token (optional)", "Token (opcional)"),
    ("Topic", "Tópico"),
    ("Server", "Servidor"),
    ("On", "Ligado"),
    (
        "Through the ntfy app (Android/iOS, free): hot GPU, failing disk, AI server down, PC off the network, undervolt done. Install ntfy and subscribe to the topic (or scan the QR code).",
        "Pelo app ntfy (Android/iOS, grátis): GPU quente, disco falhando, servidor de IA caiu, PC saiu da rede, undervolt pronto. Instale o ntfy e assine o tópico (ou leia o QR code).",
    ),
    (
        "• Advanced settings: /etc/numapilot.toml (rules) and /etc/gfx906-rocm.conf (lab/ROCm).",
        "• Configurações avançadas: /etc/numapilot.toml (regras) e /etc/gfx906-rocm.conf (laboratório/ROCm).",
    ),
    (
        "• Lossless Scaling (lsfg-vk): the official one generates the frames on the same GPU as the game.",
        "• Lossless Scaling (lsfg-vk): o oficial gera os quadros na mesma GPU do jogo.",
    ),
    (
        "• Game on an MI50: choose \"MI50 — game/graphics\" in the Programs tab.",
        "• Jogo numa MI50: escolha \"MI50 — jogo/gráfico\" na aba Programas.",
    ),
    ("Tips", "Dicas"),
    (
        "compute; also runs games through PRIME",
        "computação; também roda jogos via PRIME",
    ),
    (
        "compute + monitor connected to it",
        "computação + monitor ligado nela",
    ),
    ("video (monitor connected)", "vídeo (monitor ligado)"),
    ("Diagnosis report", "Relatório de diagnóstico"),
    (
        "Saves ~/numapilot-relatorio.txt with the diagnosis and system information",
        "Salva ~/numapilot-relatorio.txt com o diagnóstico e as informações do sistema",
    ),
    ("Generate report to send", "Gerar relatório para enviar"),
    (
        "Saves ~/numapilot-backup-DATA.tar.gz with profiles, machine settings, fans, GPUs, lab and kernel parameters. Keep it off the system disk; to restore: ./install.sh --restore FILE",
        "Salva ~/numapilot-backup-DATA.tar.gz com perfis, ajustes da máquina, ventoinhas, GPUs, laboratório e parâmetros do kernel. Guarde fora do disco do sistema; para restaurar: ./install.sh --restore ARQUIVO",
    ),
    ("Back up the settings", "Fazer backup das configurações"),
    ("Check everything", "Verificar tudo"),
    (
        "Tests each part on your hardware (NUMA, cgroups, Vulkan GPU selection, ROCm, fans, OverDrive, ReBAR, Steam) and says what to fix.",
        "Testa cada parte no seu hardware (NUMA, cgroups, seleção de GPU no Vulkan, ROCm, ventoinhas, OverDrive, ReBAR, Steam) e diz o que corrigir.",
    ),
    (
        "Lives in /etc/modprobe.d/numapilot.conf and the boot image is rebuilt: works for any kernel, even after updates. Asks for the password.",
        "Fica em /etc/modprobe.d/numapilot.conf e a imagem de boot é refeita: vale para qualquer kernel, inclusive depois de atualizar. Pede a senha.",
    ),
    (
        "Set on the kernel command line (amdgpu.ppfeaturemask): change it in the bootloader or remove it there to control it from here.",
        "Definido na linha do kernel (amdgpu.ppfeaturemask): mude no bootloader ou remova de lá para controlar por aqui.",
    ),
    ("turns off after reboot", "desliga ao reiniciar"),
    ("turns on after reboot", "liga ao reiniciar"),
    (
        "OverDrive of the AMD cards (undervolt/overclock)",
        "OverDrive das placas AMD (undervolt/overclock)",
    ),
    ("Boot options", "Opções de boot"),
    ("Weekly update", "Atualização semanal"),
    (
        "Check every week (lab, llama.cpp and ROCm)",
        "Verificar toda semana (laboratório, llama.cpp e ROCm)",
    ),
    ("Updates", "Atualizações"),
    (
        "Uncheck what you do not use on this PC to make the menu shorter. Nothing is turned off: it is only the menu.",
        "Desmarque o que você não usa neste PC para o menu ficar mais curto. Nada é desligado: é só o menu.",
    ),
    (
        "Tray icon no longer starts by itself",
        "Ícone na bandeja não abre mais sozinho",
    ),
    ("Tray icon enabled", "Ícone na bandeja ativado"),
    (
        "Tray icon when the session starts (shortcuts to the lab, AI and this window)",
        "Ícone na bandeja ao iniciar a sessão (atalhos para o laboratório, IA e esta janela)",
    ),
    (
        "stopped — sudo systemctl enable --now numapilot",
        "parado — sudo systemctl enable --now numapilot",
    ),
    ("numapilot service:", "Serviço numapilot:"),
    (
        "Added: opens at the next login",
        "Adicionado: abre no próximo login",
    ),
    (
        "(useful to let the network or another program come up first)",
        "(útil para deixar a rede ou outro programa subir antes)",
    ),
    ("Wait before opening:", "Esperar antes de abrir:"),
    (
        "For AI on the server's GPUs, use numapilot run: numapilot run --gpu compute -- llama-server -m modelo.gguf",
        "Para IA nas GPUs do servidor, use numapilot run: numapilot run --gpu compute -- llama-server -m modelo.gguf",
    ),
    ("Command", "Comando"),
    (
        "If it is not executable: the script runs through sh; AppImage and binary are made executable.",
        "Se não for executável: o script roda pelo sh; AppImage e binário ficam executáveis.",
    ),
    ("Choose…", "Escolher…"),
    (
        "/path/to/program.AppImage or script.sh",
        "/caminho/do/programa.AppImage ou script.sh",
    ),
    (
        "Your programs open now (the same as in the Programs tab):",
        "Programas seus abertos agora (os mesmos da aba Programas):",
    ),
    (
        "🔍 program name (Steam, ZapZap...)",
        "🔍 nome do programa (Steam, ZapZap...)",
    ),
    ("Open at login: command", "Abrir no login: comando"),
    ("Open at login: file", "Abrir no login: arquivo"),
    (
        "Open at login: program open now",
        "Abrir no login: programa aberto agora",
    ),
    (
        "Open at login: installed program",
        "Abrir no login: programa instalado",
    ),
    ("Uninstall", "Desinstalar"),
    (
        "The program is open: it will be closed first.",
        "O programa está aberto: ele vai ser encerrado antes.",
    ),
    ("Exact commands", "Comandos exatos"),
    (
        "Also delete the settings and the program's folder",
        "Apagar também as configurações e a pasta do programa",
    ),
    (
        "It will run, in a terminal (whatever needs the password asks there):",
        "Vai fazer, num terminal (o que pede senha pede lá):",
    ),
    ("Last log lines", "Últimas linhas do log"),
    ("Uninstall…", "Desinstalar…"),
    ("View in the Programs tab", "Ver na aba Programas"),
    ("not running", "não está rodando"),
    ("Program folder", "Pasta do programa"),
    ("Size", "Tamanho"),
    ("Description", "Descrição"),
    ("Source", "Origem"),
    ("Started by", "Abre por"),
    ("Open folder", "Abrir pasta"),
    ("no longer starts", "não abre mais"),
    (
        "Removes the program the right way for its source (package, Flatpak, npm, pipx, file...)",
        "Remove o programa do jeito certo para a origem dele (pacote, Flatpak, npm, pipx, arquivo...)",
    ),
    (
        "Deletes this startup shortcut (does not uninstall the program)",
        "Apaga este atalho de inicialização (não desinstala o programa)",
    ),
    ("Remove", "Remover"),
    (
        "Where it comes from: file, program, package, folders",
        "De onde vem: arquivo, programa, pacote, pastas",
    ),
    ("Details", "Detalhes"),
    (
        "Turn on at the next login/boot",
        "Ligar no próximo login/boot",
    ),
    (
        "Turn off at the next login/boot",
        "Desligar no próximo login/boot",
    ),
    ("Any command line", "Qualquer linha de comando"),
    ("Command…", "Comando…"),
    ("Picks the file", "Escolhe o arquivo"),
    (
        "File (AppImage, .sh script, binary)…",
        "Arquivo (AppImage, script .sh, binário)…",
    ),
    (
        "One of the programs in the Programs tab",
        "Um dos programas da aba Programas",
    ),
    ("Program open now…", "Programa aberto agora…"),
    (
        "Searches the menu programs",
        "Procura nos programas do menu",
    ),
    ("Installed program…", "Programa instalado…"),
    ("+ Add", "+ Adicionar"),
    (
        "Turning off only applies to the next login or boot: what is open stays open.",
        "Desligar só vale para o próximo login ou boot: o que está aberto continua aberto.",
    ),
    (
        "Tip: choose the GPU that has the game's monitor, if it encodes H.264/HEVC. For games on the MI50 \
             the image is captured from it; for less delay, keep the game and the encoding on the same card.",
        "Dica: escolha a GPU que tem o monitor do jogo, se ela codificar H.264/HEVC. Para jogos na MI50 \
             a imagem é capturada dela; para menos atraso, deixe o jogo e a codificação na mesma placa.",
    ),
    (
        "Software: ultrafast uses less CPU (worse image); veryfast looks better but is heavier. Start with superfast at 1080p60.",
        "Software: ultrafast gasta menos CPU (imagem pior); veryfast fica mais bonito mas pesa. Comece em superfast a 1080p60.",
    ),
    (
        "Speed saved; restart Sunshine",
        "Velocidade salva; reinicie o Sunshine",
    ),
    ("default (superfast)", "padrão (superfast)"),
    (
        "Encoder saved; restart Sunshine",
        "Codificador salvo; reinicie o Sunshine",
    ),
    ("Encoder", "Codificador"),
    (
        "Encoding GPU saved; restart Sunshine to apply",
        "GPU de codificação salva; reinicie o Sunshine para valer",
    ),
    ("monitor connected", "monitor ligado"),
    ("Sunshine chooses", "o Sunshine escolhe"),
    ("GPU that encodes the video", "GPU que codifica o vídeo"),
    ("Sunshine panel", "Painel do Sunshine"),
    ("Turning Sunshine on", "Ligando o Sunshine"),
    ("Restarting Sunshine", "Reiniciando o Sunshine"),
    ("Turning Sunshine off", "Desligando o Sunshine"),
    (
        "or from the AUR: paru -S sunshine (or your fork)",
        "ou pelo AUR: paru -S sunshine (ou o seu fork)",
    ),
    ("Installing Sunshine", "Instalando o Sunshine"),
    ("Install (Flatpak)", "Instalar (Flatpak)"),
    (
        "Sunshine is not installed.",
        "O Sunshine não está instalado.",
    ),
    (
        "Play on the phone, laptop or TV with Moonlight. The game runs here and a GPU (or the CPU) \
             encodes the video.",
        "Jogue no celular, notebook ou TV pelo Moonlight. O jogo roda aqui e uma GPU (ou a CPU) \
             codifica o vídeo.",
    ),
    ("Game streaming (Sunshine)", "Streaming de jogos (Sunshine)"),
    ("Quit the icon", "Sair do ícone"),
    ("Open numapilot", "Abrir numapilot"),
    ("Could not open numapilot", "Não consegui abrir o numapilot"),
    ("none", "nenhuma"),
    ("power saving", "economia"),
    ("high", "alta"),
    ("low", "baixa"),
    ("clear", "limpar"),
    ("all", "todos"),
    ("best", "melhor"),
    ("limit", "limitar"),
    ("speed", "velocidade"),
    ("fixed", "fixa"),
    ("curve", "curva"),
    ("missing", "falta"),
    ("recommended", "recomendado"),
    ("offset", "deslocar"),
    ("lock", "travar"),
    ("customize", "personalizar"),
    ("filter:", "filtrar:"),
    ("running", "rodando"),
    ("copy", "copiar"),
    ("(choose)", "(escolha)"),
    ("reload", "recarregar"),
    ("open", "abrir"),
    ("off", "desligado"),
    ("on", "ligado"),
    ("add", "adicionar"),
    ("remove", "remover"),
    ("from", "das"),
    ("fix", "corrigir"),
    ("yes", "sim"),
    ("choose", "escolha"),
    ("turn on", "ligar"),
    ("turns on", "liga"),
    ("scheduled", "agendado"),
    ("speed:", "velocidade:"),
    ("performance", "desempenho"),
    ("edge", "borda"),
];
