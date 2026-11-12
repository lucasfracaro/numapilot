//! Tabela português → inglês da interface (os textos com valores no meio usam `tf!` no próprio código).

pub const EN: &[(&str, &str)] = &[
    ("Programas", "Programs"),
    ("Inicialização", "Startup"),
    ("Serviços", "Services"),
    ("Mapa e medições", "Map & measurements"),
    ("Escolhas salvas", "Saved choices"),
    ("Placas de vídeo", "Graphics cards"),
    ("Máquina", "Machine"),
    ("Laboratório", "Lab"),
    ("Servidor de IA", "AI server"),
    ("Streaming", "Streaming"),
    ("Configurações", "Settings"),
    ("Testes", "Tests"),
    (
        "O que está aberto agora e em qual processador e placa cada um roda",
        "What is open now and which processor and card each one runs on",
    ),
    (
        "Tudo que abre quando o sistema liga: programas do login e serviços",
        "Everything that starts with the system: login programs and services",
    ),
    (
        "Serviços do sistema e do usuário: iniciar, parar, reiniciar e ver o log",
        "System and user services: start, stop, restart and read the log",
    ),
    (
        "Núcleos, memória por socket e medições de desempenho",
        "Cores, memory per socket and performance measurements",
    ),
    (
        "Escolhas por programa que valem toda vez que ele abrir",
        "Per-program choices applied every time it starts",
    ),
    (
        "Ventoinha, potência, clock e undervolt de cada placa",
        "Fan, power, clock and undervolt of each card",
    ),
    (
        "Modo de energia, processador, notebook e saúde do hardware",
        "Power mode, processor, laptop and hardware health",
    ),
    (
        "Containers com ROCm, Jupyter e IDEs",
        "Containers with ROCm, Jupyter and IDEs",
    ),
    (
        "Modelos locais com llama.cpp",
        "Local models with llama.cpp",
    ),
    (
        "Sunshine e Moonlight: jogar de outro aparelho",
        "Sunshine and Moonlight: play from another device",
    ),
    (
        "Outros PCs, avisos, atualização e máquina virtual",
        "Other PCs, notifications, updates and virtual machine",
    ),
    (
        "Roteiro para testar o programa, autoteste e relatório para enviar",
        "Manual test script, self-test and a report to send",
    ),
    ("Serviço ativo", "Service running"),
    ("Serviço parado", "Service stopped"),
    ("Ativar o serviço", "Start the service"),
    (
        "O serviço do numapilot não está rodando: as escolhas não são aplicadas.",
        "The numapilot service is not running: choices are not applied.",
    ),
    ("Último comando", "Last command"),
    ("Tema", "Theme"),
    ("Sistema", "System"),
    ("Claro", "Light"),
    ("Escuro", "Dark"),
    ("Memória", "Memory"),
    ("Nome", "Name"),
    ("Disco", "Disk"),
    ("Memória da GPU", "GPU memory"),
    ("Placa", "Card"),
    ("Ajustar…", "Adjust…"),
    ("Encerrar", "End"),
    ("Forçar parada", "Force stop"),
    ("Abrir tudo", "Expand all"),
    ("Fechar tudo", "Collapse all"),
    ("Só os meus", "Only mine"),
    ("🔍 Buscar app instalado…", "🔍 Find installed app…"),
    ("Buscar app instalado", "Find installed app"),
    ("Aplicativos", "Applications"),
    ("Sessão e sistema", "Session and system"),
    ("programas em execução", "programs running"),
    ("Abas do menu", "Menu tabs"),
    ("Idioma", "Language"),
    ("sem codificação de vídeo", "no video encoding"),
    (
        "instale libva-utils para verificar",
        "install libva-utils to check",
    ),
    ("nota (opcional)", "note (optional)"),
    (
        "O que aconteceu? (aparece no relatório)",
        "What happened? (goes into the report)",
    ),
    ("Falhou", "Failed"),
    ("Pulei", "Skipped"),
    (
        "Só aparecem os testes que valem para esta máquina. As respostas ficam salvas; marque e escreva uma nota quando algo der errado.",
        "Only the tests that apply to this machine are shown. Answers are saved; mark and write a note when something goes wrong.",
    ),
    ("Limpar respostas", "Clear answers"),
    (
        "Um arquivo com suas respostas, o último autoteste e o final dos registros, para enviar.",
        "A file with your answers, the last self-test and the end of the logs, to send.",
    ),
    ("Gerar relatório", "Generate report"),
    ("Autoteste", "Self-test"),
    (
        "Testa todas as partes pela linha de comando (só lê, não muda nada). Leva um ou dois minutos.",
        "Tests every part from the command line (read-only, changes nothing). Takes a minute or two.",
    ),
    ("Rodar o autoteste", "Run the self-test"),
    ("PULEI", "SKIPPED"),
    ("FALHOU", "FAILED"),
    (
        "Os arquivos aparecem na pasta de cópias.",
        "The files show up in the backup folder.",
    ),
    (
        "Use \"Guardar de todos\" em Outros PCs.",
        "Use \"Save from all\" in Other PCs.",
    ),
    ("Cópia das configurações", "Settings backup"),
    ("Outros", "Other"),
    (
        "O aviso chega no celular.",
        "The notification arrives on the phone.",
    ),
    (
        "Configure o ntfy e envie o teste.",
        "Set up ntfy and send the test.",
    ),
    ("Avisos no celular", "Phone notifications"),
    (
        "Ele aparece online; o comando de ligar acorda o PC.",
        "It shows as online; the wake command turns the PC on.",
    ),
    (
        "Adicione o link de outro PC e use \"ligar pela rede\" com ele desligado.",
        "Add another PC's link and use \"wake over the network\" with it turned off.",
    ),
    ("Outros PCs", "Other PCs"),
    ("Conecta e o jogo abre.", "It connects and the game opens."),
    (
        "Ligue o Sunshine e conecte pelo Moonlight; abra um jogo pelo celular.",
        "Turn on Sunshine and connect with Moonlight; open a game from the phone.",
    ),
    (
        "O container sobe e o Jupyter abre no navegador.",
        "The container starts and Jupyter opens in the browser.",
    ),
    (
        "Crie um container e abra o Jupyter.",
        "Create a container and open Jupyter.",
    ),
    (
        "Responde; \"Ajustar melhor\" termina e sugere configurações.",
        "It answers; \"Tune better\" finishes and suggests settings.",
    ),
    (
        "Inicie um modelo pequeno e mande uma pergunta.",
        "Start a small model and send a question.",
    ),
    (
        "Conecta usando a codificação por software, com imagem estável.",
        "It connects using software encoding, with a stable image.",
    ),
    (
        "Ligue o Sunshine pelo numapilot e conecte pelo Moonlight.",
        "Turn on Sunshine from numapilot and connect with Moonlight.",
    ),
    (
        "Streaming sem codificador de vídeo",
        "Streaming without a video encoder",
    ),
    (
        "Mostra os núcleos e CUs ativos esperados para o desbloqueio.",
        "Shows the active cores and CUs expected for the unlock.",
    ),
    ("Rode o autoteste.", "Run the self-test."),
    ("Núcleos e CUs", "Cores and CUs"),
    (
        "Depois de reiniciar, o autoteste mostra o valor novo em \"Memória da GPU (TTM)\".",
        "After rebooting, the self-test shows the new value under \"Memória da GPU (TTM)\".",
    ),
    (
        "Mude a memória da GPU e reinicie.",
        "Change the GPU memory and reboot.",
    ),
    (
        "Os apps da comunidade aparecem com instalar/abrir.",
        "The community apps show up with install/open.",
    ),
    (
        "Em Placas de vídeo, veja a seção da BC-250.",
        "In Graphics cards, look at the BC-250 section.",
    ),
    ("Apps da comunidade", "Community apps"),
    (
        "O notebook responde (ventoinha/perfil mudam) e o valor fica salvo.",
        "The laptop responds (fan/profile change) and the value stays saved.",
    ),
    (
        "Mude o perfil/ventoinha da Acer em Máquina. Se não aparece nada, rode numapilot autoteste: o item Acer mostra o motivo.",
        "Change the Acer profile/fan in Machine. If nothing shows up, run numapilot autoteste: the Acer item shows why.",
    ),
    ("Controles da Acer", "Acer controls"),
    ("Notebook", "Laptop"),
    (
        "O valor novo aparece no ryzenadj e continua depois de alguns minutos.",
        "The new value shows in ryzenadj and is still there after a few minutes.",
    ),
    (
        "Mude o limite STAPM em Máquina e rode: sudo ryzenadj --info",
        "Change the STAPM limit in Machine and run: sudo ryzenadj --info",
    ),
    ("Limites do Ryzen", "Ryzen limits"),
    (
        "Todas dão imagem; trocar de porta não deixa as telas piscando.",
        "All of them show an image; changing ports does not leave the displays flickering.",
    ),
    (
        "Ligue monitores no HDMI e no USB-C/Thunderbolt; troque um de porta.",
        "Connect monitors to HDMI and USB-C/Thunderbolt; move one to another port.",
    ),
    ("Telas externas", "External displays"),
    (
        "Nada que abre com o sistema aparece segurando a NVIDIA.",
        "Nothing that starts with the system shows up holding the NVIDIA.",
    ),
    (
        "Depois de entrar na sessão, rode: numapilot hybrid",
        "After logging in, run: numapilot hybrid",
    ),
    (
        "Programas do login na integrada",
        "Login programs on the integrated GPU",
    ),
    (
        "Lista só programas que realmente usam a NVIDIA (sem systemd, kwin, etc.).",
        "Lists only programs that really use the NVIDIA (no systemd, kwin, etc.).",
    ),
    ("Rode: numapilot hybrid", "Run: numapilot hybrid"),
    ("Quem segura a NVIDIA", "Who is holding the NVIDIA"),
    (
        "Na bateria a NVIDIA desliga (numapilot hybrid: \"desligada\"); na tomada volta.",
        "On battery the NVIDIA turns off (numapilot hybrid: \"desligada\"); on AC power it comes back.",
    ),
    (
        "Modo auto. Na bateria, feche o que usa a NVIDIA; depois ligue na tomada.",
        "Auto mode. On battery, close whatever uses the NVIDIA; then plug in the charger.",
    ),
    ("NVIDIA no automático", "NVIDIA on automatic"),
    (
        "Em até 10 s o numapilot mostra \"na bateria\" e depois \"na tomada\".",
        "Within 10 s numapilot shows \"on battery\" and then \"on AC power\".",
    ),
    (
        "Tire o carregador; depois ligue de novo.",
        "Unplug the charger; then plug it back in.",
    ),
    ("Tomada e bateria", "AC power and battery"),
    (
        "Discos, memória e placas aparecem com estado (sem \"nunca coletado\").",
        "Disks, memory and cards show a status (no \"never collected\").",
    ),
    (
        "Espere 1 minuto depois de iniciar o serviço e abra a seção Saúde.",
        "Wait 1 minute after starting the service and open the Health section.",
    ),
    ("Saúde do hardware", "Hardware health"),
    (
        "Hoje/mês começam a contar e crescem com o uso.",
        "Today/month start counting and grow with usage.",
    ),
    (
        "Informe o preço do kWh e espere alguns minutos.",
        "Enter the kWh price and wait a few minutes.",
    ),
    ("Energia e custo", "Energy and cost"),
    (
        "Na hora, o modo troca sozinho; depois do fim, volta.",
        "At that time the mode switches by itself; after the end, it goes back.",
    ),
    (
        "Crie um horário que começa daqui a 2 minutos.",
        "Create a schedule that starts 2 minutes from now.",
    ),
    ("Modo por horário", "Scheduled mode"),
    (
        "O governador e a preferência de energia mudam (numapilot autoteste mostra); Normal volta ao que era.",
        "The governor and the energy preference change (numapilot autoteste shows it); Normal restores what it was.",
    ),
    (
        "Troque entre Jogo, IA, Silencioso e Normal.",
        "Switch between Game, AI, Quiet and Normal.",
    ),
    ("Modos da máquina", "Machine modes"),
    (
        "Com carga, o clock da placa não passa do valor escolhido; Desfazer volta ao de fábrica.",
        "Under load, the card's clock stays under the chosen value; Undo restores the factory value.",
    ),
    (
        "Na placa AMD, em \"Clock e voltagem\", marque personalizar, baixe \"GPU máx.\" e aplique; depois Desfazer.",
        "On the AMD card, under \"Clock and voltage\", check customize, lower \"GPU max\" and apply; then Undo.",
    ),
    ("Clock e voltagem (AMD)", "Clock and voltage (AMD)"),
    ("Placas", "Cards"),
    (
        "Depois de reiniciar, os controles de clock e voltagem da placa ficam disponíveis.",
        "After rebooting, the card's clock and voltage controls become available.",
    ),
    (
        "Use \"Ativar OverDrive\" e reinicie o PC.",
        "Use \"Enable OverDrive\" and reboot the PC.",
    ),
    ("OverDrive (AMD)", "OverDrive (AMD)"),
    (
        "O valor aparece aplicado e volta sozinho depois de reiniciar o serviço.",
        "The value shows as applied and resets by itself after restarting the service.",
    ),
    (
        "Aplique um deslocamento pequeno (ex.: +50 MHz) e reinicie o serviço.",
        "Apply a small offset (e.g. +50 MHz) and restart the service.",
    ),
    ("Deslocamento de clock (NVIDIA)", "Clock offset (NVIDIA)"),
    (
        "Escolhe um programa de carga com 80% de uso ou mais, mostra clock e uso, termina e grava; o terminal fecha com Enter.",
        "It picks a load program with 80% usage or more, shows clock and usage, finishes and saves; the terminal closes with Enter.",
    ),
    (
        "Na tomada, use \"Achar o undervolt sozinho\" e deixe o PC parado.",
        "On AC power, use \"Find the undervolt automatically\" and leave the PC idle.",
    ),
    ("Undervolt automático", "Automatic undervolt"),
    (
        "Os watts no topo não passam do limite; Desfazer volta ao padrão.",
        "The watts at the top stay under the limit; Undo restores the default.",
    ),
    (
        "Baixe o limite de potência de uma placa e rode um jogo. Se a placa não mostra esse controle, o fabricante trava: marque Pulei.",
        "Lower a card's power limit and run a game. If the card does not show this control, the vendor locks it: mark Skipped.",
    ),
    ("Limite de potência", "Power limit"),
    (
        "A ventoinha segue a curva; Desfazer volta para o automático da placa.",
        "The fan follows the curve; Undo goes back to the card's automatic mode.",
    ),
    (
        "Em Placas de vídeo, escolha \"curva\", aplique e depois Desfazer.",
        "In Graphics cards, choose \"curve\", apply and then Undo.",
    ),
    ("Curva da ventoinha", "Fan curve"),
    (
        "A tabela é preenchida sem erro.",
        "The table fills in with no error.",
    ),
    ("Clique em \"Medir agora\".", "Click \"Measure now\"."),
    ("Medir latência e banda", "Measure latency and bandwidth"),
    ("Mapa", "Map"),
    (
        "Em \"Alocações no outro socket\" esse programa não aumenta as alocações remotas.",
        "Under \"Allocations on the other socket\" this program does not increase remote allocations.",
    ),
    (
        "Em Ajustar… de um programa, escolha usar só a memória do socket dele.",
        "In a program's Adjust…, choose to use only its socket's memory.",
    ),
    ("Memória só do socket", "Socket-only memory"),
    (
        "Acompanha a temperatura real (sobe com carga, desce parado).",
        "It follows the real temperature (up under load, down when idle).",
    ),
    (
        "Olhe o gráfico de temperatura com carga e depois parado.",
        "Watch the temperature chart under load and then idle.",
    ),
    ("Temperatura do processador", "Processor temperature"),
    (
        "Os gráficos dos threads em uso sobem; ao passar o mouse mostra o núcleo e a frequência.",
        "The charts of the threads in use go up; hovering shows the core and the frequency.",
    ),
    (
        "Abra Mapa e medições → Gráfico por thread e rode algo pesado.",
        "Open Map & measurements → Per-thread chart and run something heavy.",
    ),
    ("Gráfico por thread", "Per-thread chart"),
    (
        "As opções de inicialização dos jogos mudam e voltam ao que eram.",
        "The games' launch options change and go back to what they were.",
    ),
    (
        "Use \"Aplicar em todos os jogos\" com a Steam fechada; depois \"Remover de todos\".",
        "Use \"Apply to all games\" with Steam closed; then \"Remove from all\".",
    ),
    ("Jogos da Steam", "Steam games"),
    (
        "O programa reabre e passa a usar a placa escolhida (o uso aparece nela).",
        "The program reopens and starts using the chosen card (its usage shows there).",
    ),
    (
        "Em Ajustar…, escolha outra placa e use Reabrir.",
        "In Adjust…, pick another card and use Reopen.",
    ),
    ("Escolher a placa", "Choose the card"),
    (
        "O programa não passa do limite (o excedente vai para o swap); o PC não trava.",
        "The program does not go past the limit (the excess goes to swap); the PC does not freeze.",
    ),
    (
        "Em Ajustar…, marque \"Limitar a memória do programa a\" 2 GB e use o programa.",
        "In Adjust…, check \"Limit the program's memory to\" 2 GB and use the program.",
    ),
    ("Limite de memória", "Memory limit"),
    (
        "No Mapa (gráfico por thread) aparece o ponto amarelo nesses núcleos e o uso fica neles.",
        "In the Map (per-thread chart) the yellow dot shows on those cores and the load stays on them.",
    ),
    (
        "Em Ajustar…, escolha núcleos específicos e salve.",
        "In Adjust…, pick specific cores and save.",
    ),
    ("Escolher núcleos", "Choose cores"),
    (
        "Encerrar fecha; Forçar parada pede confirmação e fecha. kwin, plasmashell e Xwayland ficam em \"Sessão e sistema\" e os botões não deixam.",
        "End closes it; Force stop asks for confirmation and closes it. kwin, plasmashell and Xwayland stay under \"Session and system\" and the buttons refuse.",
    ),
    (
        "Abra um programa seu (ex.: Dolphin), selecione e use Encerrar; abra de novo e use Forçar parada.",
        "Open one of your programs (e.g. Dolphin), select it and use End; open it again and use Force stop.",
    ),
    ("Encerrar e Forçar parada", "End and Force stop"),
    (
        "A ordem muda; CPU, memória e GPU de um jogo batem com o Mission Center ou o MangoHud.",
        "The order changes; CPU, memory and GPU of a game match Mission Center or MangoHud.",
    ),
    (
        "Clique nos títulos Nome, CPU, Memória e GPU (de novo inverte); busque por nome e por PID.",
        "Click the Name, CPU, Memory and GPU headers (again to reverse); search by name and by PID.",
    ),
    ("Ordenar e buscar", "Sort and search"),
    (
        "Ele aparece em Programas em até 5 segundos, com o socket e a placa que está usando.",
        "It shows up in Programs within 5 seconds, with the socket and the card it is using.",
    ),
    (
        "Abra um programa qualquer (navegador, jogo).",
        "Open any program (browser, game).",
    ),
    ("Lista de programas", "Program list"),
    (
        "Mostra o que foi feito (serviço, programa e comandos) com data e hora.",
        "Shows what was done (service, program and commands) with date and time.",
    ),
    (
        "No terminal: numapilot log",
        "In the terminal: numapilot log",
    ),
    ("Registro", "Log"),
    ("Geral", "General"),
    (
        "Aparece \"Serviço parado\" com o botão; o botão pede a senha e volta para \"Serviço ativo\".",
        "\"Service stopped\" shows up with the button; the button asks for the password and it goes back to \"Service running\".",
    ),
    (
        "No terminal: sudo systemctl stop numapilot. Depois use o botão \"Ativar o serviço\".",
        "In the terminal: sudo systemctl stop numapilot. Then use the \"Start the service\" button.",
    ),
    ("Serviço parado e religar", "Service stopped and restart"),
    (
        "Os valores sobem, as barras deslizam sem pular e batem com outro monitor (Mission Center, nvtop).",
        "The values go up, the bars slide without jumping and match another monitor (Mission Center, nvtop).",
    ),
    (
        "Abra um jogo ou benchmark e olhe os cartões de CPU, memória e placas.",
        "Open a game or benchmark and watch the CPU, memory and card tiles.",
    ),
    ("Medidores do topo", "Top meters"),
    (
        "O tema muda na hora e continua o escolhido depois de reabrir.",
        "The theme changes right away and stays as chosen after reopening.",
    ),
    (
        "Troque para Claro e Escuro no menu lateral, feche e abra de novo.",
        "Switch to Light and Dark in the side menu, close and open again.",
    ),
    (
        "A janela abre. Só existe um ícone do numapilot na bandeja.",
        "The window opens. There is only one numapilot icon in the tray.",
    ),
    (
        "Ligue o ícone na bandeja (Configurações), feche a janela e clique no ícone.",
        "Turn on the tray icon (Settings), close the window and click the icon.",
    ),
    ("Ícone na bandeja", "Tray icon"),
    (
        "A janela que já estava aberta vem para frente; não abre outra.",
        "The window that was already open comes to the front; no second one opens.",
    ),
    (
        "Com a janela aberta, abra o numapilot de novo pelo menu.",
        "With the window open, open numapilot again from the menu.",
    ),
    ("Uma janela só", "Single window"),
    (
        "A janela abre em poucos segundos, no tema do sistema, sem aviso de erro.",
        "The window opens within a few seconds, in the system theme, with no error message.",
    ),
    (
        "Abra o numapilot pelo menu do sistema.",
        "Open numapilot from the system menu.",
    ),
    ("Abrir o programa", "Open the program"),
    (
        "Trazer a memória para o socket certo",
        "Bring the memory to the right socket",
    ),
    ("Aplicar", "Apply"),
    (
        "Valem na hora: processadores, núcleos, memória (exceto intercalar), limite, prioridade e exclusivo. \
                         Valem ao reabrir: placa de vídeo, intercalar e o que o programa enxerga.",
        "Apply right away: processors, cores, memory (except interleave), limit, priority and exclusive. \
                         Apply on reopen: graphics card, interleave and what the program sees.",
    ),
    ("só do socket", "socket only"),
    (
        "O gamescope usa a mesma placa escolhida acima. Bom para rodar o jogo em resolução menor e ampliar com FSR, ou travar o FPS.",
        "gamescope uses the same card chosen above. Good for running the game at a lower resolution and upscaling with FSR, or capping the FPS.",
    ),
    ("Ampliar com FSR", "Upscale with FSR"),
    ("Tela cheia", "Fullscreen"),
    ("Limite de FPS", "FPS limit"),
    ("Resolução de saída", "Output resolution"),
    ("desmarcado = a da tela", "unchecked = the display's"),
    ("Resolução do jogo", "Game resolution"),
    (
        "(não instalado: sudo pacman -S gamescope)",
        "(not installed: sudo pacman -S gamescope)",
    ),
    ("Abrir dentro do gamescope", "Open inside gamescope"),
    (
        "Clique num núcleo para usar os 2 threads; botão direito = só 1 thread.",
        "Click a core to use both threads; right-click = only 1 thread.",
    ),
    ("mesma GPU do jogo", "same GPU as the game"),
    ("mesma GPU", "same GPU"),
    ("vídeo", "video"),
    ("computação (IA/ROCm)", "compute (AI/ROCm)"),
    ("3D tela cheia (jogo)", "3D fullscreen (game)"),
    ("não mexer", "leave as is"),
    ("Perfil de energia da placa:", "Card power profile:"),
    ("Placa de vídeo", "Graphics card"),
    ("Silencioso", "Quiet"),
    ("IA", "AI"),
    ("Jogo", "Game"),
    ("não trocar", "do not switch"),
    (
        "Enquanto o programa está aberto a máquina fica neste modo (aba Máquina) e volta ao anterior quando ele fecha.",
        "While the program is open the machine stays in this mode (Machine tab) and goes back to the previous one when it closes.",
    ),
    ("Modo da máquina", "Machine mode"),
    (
        "Quanto menos núcleos, mais alto o clock. A tabela deste processador aparece com o serviço rodando (só Intel).",
        "The fewer cores, the higher the clock. This processor's table shows up with the service running (Intel only).",
    ),
    (
        "O turbo conta núcleos acordados, não threads: o clock máximo é o mesmo com ou sem. \
                                     Desmarcado, cada thread do programa tem um núcleo inteiro (melhor para a maioria dos jogos). \
                                     Marcado, rende mais em programas que usam todos os threads (compilação, renderização).",
        "Turbo counts awake cores, not threads: the maximum clock is the same either way. \
                                     Unchecked, each program thread gets a whole core (better for most games). \
                                     Checked, it does better in programs that use every thread (compiling, rendering).",
    ),
    (
        "usar os dois threads de cada núcleo",
        "use both threads of each core",
    ),
    ("Núcleos no turbo", "Cores at turbo"),
    (
        "Enquanto ele roda: os núcleos dele não entram nos estados de economia profundos e o uncore do socket \
                             fica no máximo (menos travadinhas). Ajuste em Máquina → Latência em jogos.",
        "While it runs: its cores do not enter the deep idle states and the socket's uncore \
                             stays at maximum (less micro-stutter). Tune it in Machine → Gaming latency.",
    ),
    ("baixa latência", "low latency"),
    (
        "Enquanto ele roda, o resto do sistema sai do processador inteiro onde ele está. \
                                 Com os outros núcleos dormindo, o clock dos núcleos dele sobe mais (veja a tabela em Mapa → Turbo). \
                                 Funciona melhor com poucos núcleos (2 a 8) e os estados de economia profundos ligados.",
        "While it runs, the rest of the system leaves the whole processor it is on. \
                                 With the other cores asleep, its cores clock higher (see the table in Map → Turbo). \
                                 Works best with few cores (2 to 8) and the deep idle states enabled.",
    ),
    ("turbo máximo", "maximum turbo"),
    (
        "Enquanto ele roda, o resto do sistema não usa esses núcleos.",
        "While it runs, the rest of the system does not use these cores.",
    ),
    ("núcleos exclusivos", "exclusive cores"),
    ("Prioridade", "Priority"),
    (
        "Teto de uso somando todos os núcleos: 50% = meio núcleo, 200% = dois núcleos. Bom para programas de fundo.",
        "Usage cap adding up all cores: 50% = half a core, 200% = two cores. Good for background programs.",
    ),
    ("Usar no máximo", "Use at most"),
    ("Limite de processador", "Processor limit"),
    (
        "Passou do limite, o sistema tira memória dele (vai para o swap) em vez de travar o PC todo.",
        "Past the limit, the system takes memory away from it (to swap) instead of freezing the whole PC.",
    ),
    (
        "Limitar a memória do programa a",
        "Limit the program's memory to",
    ),
    (
        "Este PC tem um processador só: toda a memória já é a mais próxima dele (as opções de memória local aparecem nos PCs com 2 processadores).",
        "This PC has a single processor: all memory is already the closest to it (the local memory options show up on PCs with 2 processors).",
    ),
    (
        "Jogos do Proton/Wine (WINE_CPU_TOPOLOGY), OpenMP, MKL, OpenBLAS, .NET, Java, Go...\n\
                             Evita jogos e o Easy Anti-Cheat recusarem abrir por ver núcleos demais. Vale ao abrir.",
        "Proton/Wine games (WINE_CPU_TOPOLOGY), OpenMP, MKL, OpenBLAS, .NET, Java, Go...\n\
                             Keeps games and Easy Anti-Cheat from refusing to start for seeing too many cores. Applies on start.",
    ),
    ("só os núcleos liberados", "only the allowed cores"),
    ("O programa enxerga", "The program sees"),
    ("escolher os núcleos no mapa", "pick the cores on the map"),
    (
        "Desligado: 1 thread por núcleo físico. Em jogos costuma dar mais FPS e menos travadas.",
        "Off: 1 thread per physical core. In games it usually gives more FPS and less stutter.",
    ),
    (
        "usar hyper-threading (2 threads por núcleo)",
        "use hyper-threading (2 threads per core)",
    ),
    ("limitar a", "limit to"),
    ("Núcleos", "Cores"),
    ("Processadores", "Processors"),
    ("Tudo automático", "All automatic"),
    (
        "4 núcleos e prioridade baixa: não atrapalha o resto",
        "4 cores and low priority: stays out of the way",
    ),
    (
        "no máximo um núcleo de processador e prioridade baixa: para launchers, sincronizadores e afins",
        "at most one processor core and low priority: for launchers, sync tools and the like",
    ),
    ("Segundo plano", "Background"),
    (
        "o socket da GPU inteiro, com os dois threads de cada núcleo, memória preferindo a local",
        "the GPU's whole socket, with both threads of each core, memory preferring local",
    ),
    ("Edição de vídeo", "Video editing"),
    (
        "4 núcleos físicos sozinhos num processador: o resto do sistema vai para o outro socket e o clock sobe ao degrau mais alto do turbo (precisa de 2 processadores; confira em Mapa → Turbo)",
        "4 physical cores alone on one processor: the rest of the system moves to the other socket and the clock rises to the highest turbo step (needs 2 processors; check Map → Turbo)",
    ),
    ("Turbo máximo", "Maximum turbo"),
    (
        "os 2 sockets, memória intercalada (banda máxima)",
        "both sockets, interleaved memory (maximum bandwidth)",
    ),
    ("Renderização", "Rendering"),
    (
        "socket da GPU de computação, memória preferindo a local",
        "compute GPU's socket, memory preferring local",
    ),
    (
        "socket da GPU, 16 núcleos físicos, memória só local, prioridade alta, núcleos reservados, o jogo enxerga só esses núcleos",
        "GPU's socket, 16 physical cores, local memory only, high priority, reserved cores, the game sees only those cores",
    ),
    ("Perfis prontos:", "Ready-made profiles:"),
    (
        "Das últimas 6 horas em que o programa esteve aberto, um ponto por minuto. Use para escolher o limite de memória e de processador com folga sobre o pico.",
        "From the last 6 hours the program was open, one point per minute. Use it to pick the memory and processor limits with headroom over the peak.",
    ),
    (
        "Abra pelo atalho do menu (numapilot run) para o teste valer.",
        "Open it from the menu shortcut (numapilot run) for the test to count.",
    ),
    ("Cancelar", "Cancel"),
    ("Recomeçar", "Start over"),
    ("Usar a melhor", "Use the best"),
    ("Abrir o jogo", "Open the game"),
    ("Todos os testes terminaram.", "All tests finished."),
    ("próximo", "next"),
    ("FPS médio", "Average FPS"),
    ("Configuração", "Setting"),
    ("Começar a otimização", "Start the optimization"),
    (
        "Precisa do MangoHud: sudo pacman -S mangohud",
        "Needs MangoHud: sudo pacman -S mangohud",
    ),
    (
        "Testa algumas configurações (núcleos, HT, socket, placa), uma a cada vez que você abre o jogo, \
                     mede com o MangoHud (FPS médio e 1% low) e mostra a melhor.",
        "Tests a few settings (cores, HT, socket, card), one each time you open the game, \
                     measures with MangoHud (average FPS and 1% low) and shows the best.",
    ),
    ("Otimizar FPS", "Optimize FPS"),
    (
        "Para instalar pela interface, abra o jogo pela Steam com o numapilot (o perfil fica ligado ao appid).",
        "To install from the interface, open the game through Steam with numapilot (the profile is tied to the appid).",
    ),
    (
        "Pasta do jogo não encontrada nas bibliotecas da Steam.",
        "Game folder not found in the Steam libraries.",
    ),
    (
        "Baixa a versão estável mais nova e copia para a pasta do executável, guardando o que for do jogo",
        "Downloads the newest stable version and copies it to the executable's folder, keeping what belongs to the game",
    ),
    ("Instalar no jogo", "Install in the game"),
    ("Reinstalar (versão nova)", "Reinstall (new version)"),
    ("Remover do jogo", "Remove from the game"),
    (
        "Não use o OptiScaler em jogos online: o anti-cheat pode banir.",
        "Do not use OptiScaler in online games: the anti-cheat may ban you.",
    ),
    (
        "Para placas sem FSR 4 oficial (RDNA 2/3.5, Intel, NVIDIA). Placas sem INT8 não ganham FSR 4.",
        "For cards without official FSR 4 (RDNA 2/3.5, Intel, NVIDIA). Cards without INT8 do not get FSR 4.",
    ),
    (
        "forçar FSR 4 INT8 (experimental)",
        "force FSR 4 INT8 (experimental)",
    ),
    (
        "mostrar a marca do FSR 4/DLSS (para conferir)",
        "show the FSR 4/DLSS watermark (to check)",
    ),
    (
        "Precisa do proton-cachyos ou do GE-Proton como ferramenta de compatibilidade do jogo.",
        "Needs proton-cachyos or GE-Proton as the game's compatibility tool.",
    ),
    ("Nenhum", "None"),
    ("não move a memória", "does not move the memory"),
    ("Livre", "Free"),
    (
        "mais banda total; bom para render/IA na CPU (vale ao reabrir)",
        "more total bandwidth; good for rendering/AI on the CPU (applies on reopen)",
    ),
    ("Intercalar entre sockets", "Interleave across sockets"),
    (
        "usa a do outro socket só quando faltar",
        "uses the other socket's only when it runs out",
    ),
    ("Preferir a local", "Prefer local"),
    (
        "menor latência; se a memória do socket encher, o programa sofre",
        "lowest latency; if the socket's memory fills up, the program suffers",
    ),
    ("Só do socket (estrito)", "Socket only (strict)"),
    (
        "o numapilot decide (normalmente: memória do socket dos núcleos)",
        "numapilot decides (normally: memory of the cores' socket)",
    ),
    ("Automático", "Automatic"),
    (
        "BC-250: com 16 GB, 14 GiB deixa os modelos de até ~14B carregarem (BIOS com UMA em 512 MB). O servidor de IA usa o Vulkan e já liga o GGML_VK_PREFER_HOST_MEMORY (+5 a 12% na geração).",
        "BC-250: with 16 GB, 14 GiB lets models up to ~14B load (BIOS with UMA at 512 MB). The AI server uses Vulkan and already sets GGML_VK_PREFER_HOST_MEMORY (+5 to 12% in generation).",
    ),
    ("padrão", "default"),
    (
        "Grava para os próximos boots e aplica agora (pede a senha)",
        "Saves for the next boots and applies now (asks for the password)",
    ),
    (
        "Quanto da RAM a GPU integrada pode usar. O padrão do kernel é a metade, o que impede modelos grandes de carregar. Deixe 1-2 GiB para o sistema.",
        "How much RAM the integrated GPU may use. The kernel default is half, which keeps large models from loading. Leave 1-2 GiB for the system.",
    ),
    ("Memória para a GPU (IA):", "Memory for the GPU (AI):"),
    ("Desativando o OverDrive", "Disabling OverDrive"),
    ("Ativando o OverDrive", "Enabling OverDrive"),
    ("Testar a escolha da placa", "Test the card selection"),
    ("Desfazer", "Undo"),
    (
        "Parada há 1 min, a placa fica no nível mais baixo de clock e voltagem até voltar a ser usada. Ajuda nas MI50, que gastam bastante paradas; o primeiro pedido depois demora uma fração de segundo a mais.",
        "After 1 min idle, the card stays at the lowest clock and voltage level until it is used again. Helps on the MI50s, which draw a lot when idle; the first request afterwards takes a fraction of a second longer.",
    ),
    ("Economia quando parada", "Power saving when idle"),
    ("Frequência máxima:", "Maximum frequency:"),
    ("padrão da placa", "card default"),
    ("Potência:", "Power:"),
    ("curva padrão", "default curve"),
    ("+ ponto", "+ point"),
    ("da placa (automática)", "card's own (automatic)"),
    (
        "(curva gravada no firmware da placa)",
        "(curve stored in the card's firmware)",
    ),
    ("Ventoinha:", "Fan:"),
    (
        "O driver não expõe controles de ventoinha/potência para esta placa.",
        "The driver does not expose fan/power controls for this card.",
    ),
    (
        "Dormindo para economizar energia: acorda sozinha quando um programa usa a placa. Os controles aparecem com ela ligada.",
        "Asleep to save power: it wakes by itself when a program uses the card. The controls show up when it is on.",
    ),
    (
        "O driver oficial da NVIDIA não está carregado: sem sensores nem controles.",
        "The official NVIDIA driver is not loaded: no sensors or controls.",
    ),
    (
        "Muda sozinho conforme o programa (Ajustar… → Perfil de energia da placa).",
        "Changes by itself depending on the program (Adjust… → Card power profile).",
    ),
    (
        "A geração cai quando a placa está parada (economia); confira com ela em uso.",
        "The generation drops when the card is idle (power saving); check it while in use.",
    ),
    (
        "Roda o vulkaninfo e o glxinfo como um jogo rodaria nesta placa e mostra qual placa respondeu (Vulkan/DirectX e OpenGL).",
        "Runs vulkaninfo and glxinfo the way a game would on this card and shows which card answered (Vulkan/DirectX and OpenGL).",
    ),
    ("Testar a escolha desta placa", "Test this card's selection"),
    ("Histórico:", "History:"),
    (
        "Temperatura, ventoinha e potência de cada placa. Quem aplica é o serviço numapilot; \
             se ele parar, as ventoinhas voltam para o controle da própria placa. Acima de 95 °C a ventoinha vai a 100%.",
        "Temperature, fan and power of each card. The numapilot service applies them; \
             if it stops, the fans go back to the card's own control. Above 95 °C the fan goes to 100%.",
    ),
    (
        "instalação manual (veja a página)",
        "manual install (see the page)",
    ),
    ("página", "page"),
    (
        "Baixa o código (git) em ~/.local/share/numapilot/comunidade e cria os comandos em ~/.local/bin, num terminal",
        "Downloads the code (git) into ~/.local/share/numapilot/comunidade and creates the commands in ~/.local/bin, in a terminal",
    ),
    ("Instalar", "Install"),
    (
        "Instala pelo AUR (paru/yay) num terminal; pode pedir a senha",
        "Installs from the AUR (paru/yay) in a terminal; may ask for the password",
    ),
    (
        "Instala pelo AUR (pacote -git do autor) num terminal; pode pedir a senha. Depois este botão vira \"Gerenciar núcleos, CUs, frequências\"",
        "Installs from the AUR (the author's -git package) in a terminal; may ask for the password. Afterwards this button becomes \"Manage cores, CUs, frequencies\"",
    ),
    (
        "Instalar o BC250 Control Center",
        "Install BC250 Control Center",
    ),
    (
        "Baixa e instala a versão mais nova de quem mantém (num terminal; pode pedir a senha)",
        "Downloads and installs the maintainer's newest version (in a terminal; may ask for the password)",
    ),
    ("Atualizar", "Update"),
    (
        "⚙ Gerenciar núcleos, CUs, frequências e voltagens",
        "⚙ Manage cores, CUs, frequencies and voltages",
    ),
    (
        "Na linha de comando: numapilot board setup",
        "On the command line: numapilot board setup",
    ),
    (
        "(seção mais abaixo nesta aba)",
        "(section further down in this tab)",
    ),
    ("Instalar (AUR)", "Install (AUR)"),
    ("Ligar (pede a senha)", "Enable (asks for the password)"),
    ("Guia da comunidade", "Community guide"),
    (
        "Instalados do código mais novo de quem mantém (AUR -git ou git); \"Atualizar\" baixa de novo.",
        "Installed from the maintainer's newest code (AUR -git or git); \"Update\" downloads it again.",
    ),
    (
        " (bc250_cc_write_mode=3): não use o desbloqueio ao vivo junto",
        " (bc250_cc_write_mode=3): do not use the live unlock together with it",
    ),
    ("Governor da GPU", "GPU governor"),
    (
        "Suba o clock e a voltagem aos poucos e teste a estabilidade (ex.: Unigine Superposition) a cada passo.",
        "Raise the clock and the voltage little by little and test the stability (e.g. Unigine Superposition) at each step.",
    ),
    (
        "Faz backup da configuração, grava a nova e reinicia o serviço; se ele não subir, volta a anterior. Pede a senha.",
        "Backs up the configuration, writes the new one and restarts the service; if it does not come up, the previous one is restored. Asks for the password.",
    ),
    (
        "Salvar e reiniciar o governor",
        "Save and restart the governor",
    ),
    (
        "Abre um terminal com o paru/yay",
        "Opens a terminal with paru/yay",
    ),
    ("Instalar o governor (AUR)", "Install the governor (AUR)"),
    (
        "Nenhum governor rodando: a GPU fica com clock fixo.",
        "No governor running: the GPU stays at a fixed clock.",
    ),
    ("ROCm da comunidade", "Community ROCm"),
    ("ROCm oficial", "Official ROCm"),
    ("geração de quadros (lsfg-vk)", "frame generation (lsfg-vk)"),
    ("FSR 4 (via vkd3d-proton)", "FSR 4 (through vkd3d-proton)"),
    ("em dia", "up to date"),
    ("Atualizando sistema e driver", "Updating system and driver"),
    (
        "No CachyOS o driver atualiza junto com o sistema (pacman -Syu). Pede a senha.",
        "On CachyOS the driver updates along with the system (pacman -Syu). Asks for the password.",
    ),
    ("Driver NVIDIA", "NVIDIA driver"),
    (
        "Atualiza o sistema e instala o driver certo para a geração da placa (pelo chwd do CachyOS). Pede a senha; depois reinicie.",
        "Updates the system and installs the right driver for the card's generation (through CachyOS's chwd). Asks for the password; then reboot.",
    ),
    ("Instalar driver oficial", "Install official driver"),
    ("Reinstalar driver oficial", "Reinstall official driver"),
    (
        "O driver instalado é mais novo que o carregado: reinicie.",
        "The installed driver is newer than the loaded one: reboot.",
    ),
    (
        "Usando o nouveau (driver aberto da comunidade): sem desempenho para jogos.",
        "Using nouveau (the community's open driver): no performance for games.",
    ),
    ("não", "no"),
    (
        "Undervolt na NVIDIA: trave o clock (ex.: 1800 MHz) e desloque a curva (+100 a +200 MHz): mesmo FPS gastando e esquentando menos. O botão \"Achar sozinho\" testa os passos por você.",
        "Undervolt on NVIDIA: lock the clock (e.g. 1800 MHz) and offset the curve (+100 to +200 MHz): same FPS using less power and heat. The \"Find automatically\" button tests the steps for you.",
    ),
    (
        "Sem travar o clock máximo, o deslocamento vira overclock (mais clock, mesma voltagem). Para undervolt, trave o clock também.",
        "Without locking the maximum clock, the offset becomes an overclock (more clock, same voltage). For undervolt, lock the clock too.",
    ),
    ("Memória:", "Memory:"),
    ("deslocar a curva", "offset the curve"),
    (
        "Desloca a curva clock/voltagem para cima: com o clock máximo travado, a placa chega nele com voltagem menor. Suba de 15 em 15 MHz e teste em jogo; se travar ou der artefato, volte um passo.",
        "Shifts the clock/voltage curve up: with the maximum clock locked, the card reaches it at a lower voltage. Go up 15 MHz at a time and test in a game; if it hangs or shows artifacts, go back one step.",
    ),
    ("Clock máximo:", "Maximum clock:"),
    (
        "precisa do vkmark: sudo pacman -S vkmark",
        "needs vkmark: sudo pacman -S vkmark",
    ),
    (
        "Testa passo a passo com carga (1 min cada) e grava o último estável com 1 passo de margem. Deixe o PC parado; leva de 10 a 30 min.",
        "Tests step by step under load (1 min each) and saves the last stable one with 1 step of margin. Leave the PC idle; takes 10 to 30 min.",
    ),
    (
        "Achar o undervolt sozinho",
        "Find the undervolt automatically",
    ),
    (
        "Se a placa travar ou o jogo fechar sozinho, volte um passo. Ao reiniciar o PC o serviço reaplica; desmarcar \"personalizar\" volta ao padrão.",
        "If the card hangs or the game closes by itself, go back one step. On reboot the service reapplies; unchecking \"customize\" restores the default.",
    ),
    (
        "Undervolt: mesma frequência com menos voltagem (menos calor e consumo). Teste a estabilidade.",
        "Undervolt: same frequency with less voltage (less heat and power). Test the stability.",
    ),
    ("curva de voltagem", "voltage curve"),
    ("negativo = undervolt", "negative = undervolt"),
    ("deslocar a voltagem", "offset the voltage"),
    ("memória máx.", "memory max"),
    ("GPU máx.", "GPU max"),
    ("Clock e voltagem:", "Clock and voltage:"),
    (
        "Grava a opção do driver amdgpu em /etc/modprobe.d e refaz a imagem de boot. \
                         Vale para qualquer kernel, inclusive depois de atualizar. Pede a senha; depois é só reiniciar.",
        "Writes the amdgpu driver option to /etc/modprobe.d and rebuilds the boot image. \
                         Works for any kernel, even after updates. Asks for the password; then just reboot.",
    ),
    ("Ativar OverDrive", "Enable OverDrive"),
    (
        "Clock/voltagem (undervolt/overclock): o OverDrive do driver está desligado.",
        "Clock/voltage (undervolt/overclock): the driver's OverDrive is off.",
    ),
    (
        "Clock/voltagem: o OverDrive liga quando você reiniciar o computador.",
        "Clock/voltage: OverDrive turns on when you reboot the computer.",
    ),
    (
        "Placas AMD: o driver já tenta aumentar sozinho no boot, mesmo com a opção desligada na BIOS. Se não conseguiu, faltou espaço \
         na janela da ponte PCI: o parâmetro pci=realloc (Máquina → Parâmetros do kernel) pede ao Linux para reorganizar.",
        "AMD cards: the driver already tries to grow it at boot, even with the BIOS option off. If it could not, there was no room \
         in the PCI bridge window: the pci=realloc parameter (Machine → Kernel parameters) asks Linux to rearrange.",
    ),
    (
        "Na NVIDIA o driver só usa o BAR grande se a BIOS ligar o Resizable BAR (e a placa for RTX 30 ou mais nova).",
        "On NVIDIA the driver only uses the large BAR if the BIOS enables Resizable BAR (and the card is an RTX 30 or newer).",
    ),
    (
        "Primeiro passo, na BIOS: ligar \"Above 4G Decoding\" (às vezes chamado de \"64-bit PCI\" ou \"MMIO above 4G\"). Sem isso não dá.",
        "First step, in the BIOS: enable \"Above 4G Decoding\" (sometimes called \"64-bit PCI\" or \"MMIO above 4G\"). It will not work without it.",
    ),
    (
        "sem histórico ainda (o serviço grava a cada 30 s)",
        "no history yet (the service records every 30 s)",
    ),
    ("memória", "memory"),
    (
        "nenhum terminal encontrado (konsole, gnome-terminal, kitty, alacritty, xterm)",
        "no terminal found (konsole, gnome-terminal, kitty, alacritty, xterm)",
    ),
    ("[terminou com erro]", "[finished with an error]"),
    ("[concluído]", "[finished]"),
    (
        "Dica: apps instalados no laboratório (apt install ... dentro do Terminal) aparecem aqui ao clicar em \
                 \"Atualizar apps\". Para guardar o que instalou mesmo após atualizar a imagem, prefira pip/conda dentro de ~/lab.",
        "Tip: apps installed in the lab (apt install ... inside the Terminal) show up here after clicking \
                 \"Refresh apps\". To keep what you installed even after updating the image, prefer pip/conda inside ~/lab.",
    ),
    (
        "Pacotes do sistema (instalados à mão)",
        "System packages (installed by hand)",
    ),
    ("Pacotes Python", "Python packages"),
    (
        "só dá para abrir apps do laboratório e de containers distrobox (que têm acesso à tela)",
        "only apps from the lab and from distrobox containers (which have display access) can be opened",
    ),
    ("nenhum app com atalho", "no app with a shortcut"),
    ("Ver apps", "View apps"),
    ("Atualizar apps", "Refresh apps"),
    ("Ligar", "Turn on"),
    ("Parar", "Stop"),
    ("servidor de IA", "AI server"),
    ("laboratório", "lab"),
    ("Nenhum container ainda.", "No containers yet."),
    (
        "clique em \"Ver apps\" para listar o que está instalado em cada um",
        "click \"View apps\" to list what is installed in each one",
    ),
    ("Pasta ~/lab", "~/lab folder"),
    ("Abrindo o PyCharm", "Opening PyCharm"),
    (
        "Precisa de LAB_PYCHARM=1 na imagem (Configurações do laboratório)",
        "Needs LAB_PYCHARM=1 in the image (Lab settings)",
    ),
    (
        "Abrindo o VS Code conectado ao laboratório",
        "Opening VS Code connected to the lab",
    ),
    (
        "Precisa da extensão Remote - SSH",
        "Needs the Remote - SSH extension",
    ),
    ("VS Code (app do PC)", "VS Code (PC app)"),
    (
        "Senha do VS Code copiada (cole na tela de login)",
        "VS Code password copied (paste it on the login screen)",
    ),
    ("VS Code (navegador)", "VS Code (browser)"),
    ("Abrir:", "Open:"),
    (
        "Voltando para a imagem anterior",
        "Rolling back to the previous image",
    ),
    ("Voltar versão", "Roll back version"),
    ("Reconstruindo o laboratório", "Rebuilding the lab"),
    ("Atualizar pacotes Python", "Update Python packages"),
    ("Atualizando o laboratório", "Updating the lab"),
    (
        "Base nova do PyTorch/ROCm; testa nas GPUs e volta sozinho se falhar",
        "New PyTorch/ROCm base; tests on the GPUs and reverts by itself if it fails",
    ),
    ("Teste das GPUs no laboratório", "GPU test in the lab"),
    ("Testar GPUs", "Test GPUs"),
    ("Ligando o laboratório", "Turning the lab on"),
    (
        "Criar e ligar (demora na 1ª vez)",
        "Create and turn on (slow the 1st time)",
    ),
    ("Desligando o laboratório", "Turning the lab off"),
    ("Desligar", "Turn off"),
    (
        "PyTorch nas MI50 + pandas, scikit-learn, transformers... Seus arquivos ficam em ~/lab.",
        "PyTorch on the MI50s + pandas, scikit-learn, transformers... Your files stay in ~/lab.",
    ),
    ("ainda não criado", "not created yet"),
    ("Laboratório de dados e IA", "Data and AI lab"),
    (
        "Instale o podman:  sudo pacman -S podman",
        "Install podman:  sudo pacman -S podman",
    ),
    ("sem podman/docker", "no podman/docker"),
    (
        "VS Code não está instalado no PC",
        "VS Code is not installed on the PC",
    ),
    (
        "Ajustes do servidor de IA salvos",
        "AI server settings saved",
    ),
    ("Abrir chat", "Open chat"),
    ("Log do servidor de IA", "AI server log"),
    ("Ver log", "View log"),
    (
        "Testa 1 ou todas as GPUs, divisão por camadas ou linhas, flash attention e cache q8_0 com o modelo escolhido e grava a combinação mais rápida. Com o servidor parado; leva alguns minutos.",
        "Tests 1 or all GPUs, layer or row split, flash attention and q8_0 cache with the chosen model and saves the fastest combination. With the server stopped; takes a few minutes.",
    ),
    ("Otimizar (tokens/s)", "Optimize (tokens/s)"),
    ("Atualizando o llama.cpp", "Updating llama.cpp"),
    ("Atualizar llama.cpp", "Update llama.cpp"),
    ("Salvar ajustes", "Save settings"),
    ("Reiniciando o servidor de IA", "Restarting the AI server"),
    ("Reiniciar com estes ajustes", "Restart with these settings"),
    ("Parando o servidor de IA", "Stopping the AI server"),
    ("Iniciando o servidor de IA", "Starting the AI server"),
    ("Iniciar", "Start"),
    (
        "ex.: --flash-attn on -ctk q8_0 -ctv q8_0",
        "e.g. --flash-attn on -ctk q8_0 -ctv q8_0",
    ),
    ("Argumentos extras", "Extra arguments"),
    ("Porta", "Port"),
    ("Contexto (tokens)", "Context (tokens)"),
    ("As duas MI50 (modelo dividido)", "Both MI50s (split model)"),
    ("Modelo", "Model"),
    ("Pasta dos modelos", "Models folder"),
    (
        "Roda modelos .gguf nas MI50 com uma API compatível com a da OpenAI (serve para Open WebUI, \
             Continue, scripts...). Cada GPU usa os processadores e a memória do próprio socket.",
        "Runs .gguf models on the MI50s with an OpenAI-compatible API (works with Open WebUI, \
             Continue, scripts...). Each GPU uses the processors and memory of its own socket.",
    ),
    ("carregando o modelo...", "loading the model..."),
    ("Servidor de IA (llama.cpp)", "AI server (llama.cpp)"),
    ("Recarregar", "Reload"),
    ("Parâmetros do kernel", "Kernel parameters"),
    (
        "Pede a senha, faz backup do arquivo do bootloader e regrava as entradas. Vale depois de reiniciar.",
        "Asks for the password, backs up the bootloader file and rewrites the entries. Applies after a reboot.",
    ),
    ("Aplicar no boot", "Apply at boot"),
    ("ex.: amd_iommu=on", "e.g. amd_iommu=on"),
    ("Outro:", "Other:"),
    (
        "Bootloader não reconhecido (Limine, systemd-boot/sdboot-manage, GRUB ou rEFInd): não dá para editar daqui.",
        "Bootloader not recognized (Limine, systemd-boot/sdboot-manage, GRUB or rEFInd): cannot be edited from here.",
    ),
    (
        "O kernel não informou vulnerabilidades (ou este processador não é afetado).",
        "The kernel reported no vulnerabilities (or this processor is not affected).",
    ),
    ("Ganho", "Gain"),
    ("Risco sem ela", "Risk without it"),
    ("Situação no seu processador", "Status on your processor"),
    (
        "Desligar todas (mitigations=off)",
        "Turn all off (mitigations=off)",
    ),
    ("Todas ligadas (padrão)", "All on (default)"),
    ("Desligar todas", "Turn all off"),
    (
        "Desligar as leves (mantém Spectre e Meltdown)",
        "Turn off the light ones (keeps Spectre and Meltdown)",
    ),
    (
        "Proteções contra ataques de canal lateral. Desligar devolve desempenho, mas deixa programas maliciosos lerem \
                 dados de outros programas ou do kernel. Valem depois de reiniciar.",
        "Protections against side-channel attacks. Turning them off gives performance back, but lets malicious programs read \
                 data from other programs or from the kernel. They apply after a reboot.",
    ),
    ("Mitigações do processador", "Processor mitigations"),
    ("Pelo IPMI (só leitura):", "Through IPMI (read-only):"),
    ("o mais quente de tudo", "the hottest of all"),
    ("GPU mais quente", "Hottest GPU"),
    ("CPU mais quente", "Hottest CPU"),
    ("Seguir:", "Follow:"),
    ("da placa", "board's own"),
    ("nome (ex.: frente, CPU 0)", "name (e.g. front, CPU 0)"),
    (
        "Nenhuma ventoinha controlável encontrada. Placas comuns precisam do driver do chip de sensores \
                     (ex.: sudo modprobe nct6775; o sensors-detect diz qual); placas de servidor usam o IPMI (sudo pacman -S ipmitool).",
        "No controllable fan found. Regular boards need the sensor chip's driver \
                     (e.g. sudo modprobe nct6775; sensors-detect tells which); server boards use IPMI (sudo pacman -S ipmitool).",
    ),
    ("Ventoinhas da placa-mãe", "Motherboard fans"),
    (
        "Quem aplica é o serviço; se ele parar, tudo volta ao que era.",
        "The service applies them; if it stops, everything goes back to what it was.",
    ),
    ("Aplicar ajustes da máquina", "Apply machine settings"),
    (
        "Ventoinhas pelo BMC (IPMI):",
        "Fans through the BMC (IPMI):",
    ),
    (
        "scx_loader não instalado (sudo pacman -S scx-scheds scx-tools).",
        "scx_loader not installed (sudo pacman -S scx-scheds scx-tools).",
    ),
    ("padrão do kernel", "kernel default"),
    (
        "padrão do kernel (desligar sched_ext)",
        "kernel default (turn sched_ext off)",
    ),
    ("Escalonador (sched_ext)", "Scheduler (sched_ext)"),
    (
        "Páginas de 2 MB separadas no boot, para programas que usam hugetlbfs (máquinas virtuais, DPDK, alguns bancos de dados). \
                     O llama.cpp e o PyTorch não usam: para eles valem THP e a memória no socket certo.",
        "2 MB pages set aside at boot, for programs that use hugetlbfs (virtual machines, DPDK, some databases). \
                     llama.cpp and PyTorch do not use them: for those, THP and memory on the right socket are what count.",
    ),
    ("Hugepages reservadas", "Reserved hugepages"),
    (
        "always: menos falhas de TLB (bom para IA e render na CPU); madvise: só quem pede (padrão).",
        "always: fewer TLB misses (good for AI and CPU rendering); madvise: only for those that ask (default).",
    ),
    (
        "Ligado: prefere liberar cache do mesmo nó a usar memória do outro. Bom para latência, ruim para cache de disco.",
        "On: prefers freeing cache on the same node over using memory from the other. Good for latency, bad for disk cache.",
    ),
    (
        "Recuperar memória no próprio nó (zone_reclaim)",
        "Reclaim memory on the same node (zone_reclaim)",
    ),
    (
        "O kernel move páginas sozinho para perto de quem usa. Com o numapilot cuidando do lugar dos programas, desligar evita migrações que causam travadinhas.",
        "The kernel moves pages by itself closer to whoever uses them. With numapilot placing the programs, turning it off avoids migrations that cause micro-stutter.",
    ),
    ("Balanceamento NUMA do kernel", "Kernel NUMA balancing"),
    ("uncore no máximo", "uncore at maximum"),
    ("Estados permitidos:", "Allowed states:"),
    (
        "Vale enquanto roda um programa com \"Baixa latência\" (o perfil Jogo liga): nos núcleos dele, o processador não entra \
                 nos estados de economia profundos (acordar deles causa travadinhas) e o uncore do socket fica no máximo. Ao fechar, volta tudo.",
        "Applies while a program with \"Low latency\" runs (the Game profile turns it on): on its cores, the processor does not enter \
                 the deep idle states (waking from them causes micro-stutter) and the socket's uncore stays at maximum. On close, everything goes back.",
    ),
    ("Latência em jogos", "Gaming latency"),
    (
        "Quanto o processador pode puxar por alguns segundos antes de cair para o limite acima.",
        "How much the processor may draw for a few seconds before dropping to the limit above.",
    ),
    ("Pico de potência (PL2)", "Power peak (PL2)"),
    ("indisponível (RAPL)", "unavailable (RAPL)"),
    (
        "indisponível (sudo modprobe intel_uncore_frequency)",
        "unavailable (sudo modprobe intel_uncore_frequency)",
    ),
    (
        "Uncore mais alto = menor latência de memória e do cache L3, à custa de alguns watts.",
        "Higher uncore = lower memory and L3 cache latency, at the cost of a few watts.",
    ),
    ("Uncore (L3 + memória)", "Uncore (L3 + memory)"),
    ("Frequência mín. / máx.", "Min / max frequency"),
    (
        "O ajuste principal do Ryzen (amd-pstate): de performance a power. Com o governor performance o driver fixa em performance.",
        "Ryzen's main knob (amd-pstate): from performance to power. With the performance governor the driver locks it to performance.",
    ),
    ("Preferência de energia (EPP)", "Energy preference (EPP)"),
    ("Viés de energia (EPB)", "Energy bias (EPB)"),
    ("Agora", "Now"),
    ("Processador", "Processor"),
    ("esta placa não suporta", "this adapter does not support it"),
    ("agora: ? (instale o ethtool)", "now: ? (install ethtool)"),
    ("agora: desligado", "now: off"),
    ("agora: ligado", "now: on"),
    (
        "Deixa este PC ser ligado pelo painel de outro (Meus PCs → Ligar). Precisa também da opção na BIOS (Wake on LAN / Power on by PCI-E) e do cabo de rede.",
        "Lets this PC be turned on from another one's panel (My PCs → Wake). Also needs the BIOS option (Wake on LAN / Power on by PCI-E) and the network cable.",
    ),
    (
        "Ligar pela rede (Wake-on-LAN)",
        "Wake over the network (Wake-on-LAN)",
    ),
    ("ERROS", "ERRORS"),
    ("SMART FALHOU", "SMART FAILED"),
    (
        "Para ver os discos: sudo pacman -S smartmontools",
        "To see the disks: sudo pacman -S smartmontools",
    ),
    ("Nenhum problema encontrado.", "No problems found."),
    (
        "O serviço coleta 1 min depois de iniciar e depois a cada 30 min.",
        "The service collects 1 min after starting and then every 30 min.",
    ),
    ("últimos 14 dias", "last 14 days"),
    (
        "O serviço mede processadores e placas de vídeo. Placa-mãe, memória, discos, ventoinhas e a perda da fonte somam uns 30-80 W; informe para a conta ficar perto da tomada (ou meça com um medidor de tomada).",
        "The service measures processors and graphics cards. Motherboard, memory, disks, fans and power supply losses add some 30-80 W; enter it so the figure gets close to the wall (or measure with a wall meter).",
    ),
    ("Resto do PC (W):", "Rest of the PC (W):"),
    ("Preço do kWh (R$):", "kWh price (R$):"),
    ("Energia", "Energy"),
    ("+ horário", "+ schedule"),
    ("horário inválido (use HH:MM)", "invalid time (use HH:MM)"),
    ("às", "to"),
    (
        "Ex.: Silencioso das 23:00 às 07:00. Programa com \"modo da máquina\" no perfil (Ajustar…) tem prioridade; trocar o modo à mão vale até a regra mudar.",
        "E.g. Quiet from 23:00 to 07:00. A program with \"machine mode\" in its profile (Adjust…) takes priority; switching the mode by hand holds until the rule changes.",
    ),
    ("Modos por horário", "Scheduled modes"),
    ("Teclado RGB", "RGB keyboard"),
    ("máximo", "maximum"),
    ("automático", "automatic"),
    (
        "Ventoinhas, limite de carga em 80% e RGB precisam do módulo da comunidade Linuwu-Sense.",
        "Fans, the 80% charge limit and RGB need the community module Linuwu-Sense.",
    ),
    (
        "Baixar o STAPM e a temperatura deixa o notebook frio e silencioso; o serviço reaplica se o firmware voltar os valores ao trocar tomada/bateria.",
        "Lowering STAPM and the temperature keeps the laptop cool and quiet; the service reapplies if the firmware resets the values when switching AC/battery.",
    ),
    ("agora ? (serviço)", "now ? (service)"),
    ("Temperatura máxima", "Maximum temperature"),
    ("Picos (PPT rápido)", "Peaks (fast PPT)"),
    ("Média curta (PPT lento)", "Short average (slow PPT)"),
    ("Sustentado (STAPM)", "Sustained (STAPM)"),
    ("instalar (AUR)", "install (AUR)"),
    (
        "Instale o ryzenadj para ajustar STAPM/PPT (o equivalente do PL1/PL2 da Intel).",
        "Install ryzenadj to tune STAPM/PPT (the equivalent of Intel's PL1/PL2).",
    ),
    ("Limites de potência do Ryzen", "Ryzen power limits"),
    (
        "Passa potência do processador para a placa de vídeo em jogos (nvidia-powerd).",
        "Shifts power from the processor to the graphics card in games (nvidia-powerd).",
    ),
    ("Dynamic Boost da NVIDIA", "NVIDIA Dynamic Boost"),
    (
        "Esses programas abriram a placa dedicada; enquanto estiverem abertos ela não dorme e gasta bateria. Abra-os na integrada (perfil do programa → placa) para economizar.",
        "These programs opened the dedicated card; while they are open it does not sleep and drains the battery. Open them on the integrated GPU (program profile → card) to save power.",
    ),
    ("sempre ligada", "always on"),
    ("dormir quando ninguém usa", "sleep when nobody uses it"),
    (
        "Perfil do fabricante (ACPI): mexe em ventoinhas e limites de potência. Os modos já escolhem um.",
        "Vendor profile (ACPI): changes fans and power limits. The modes already pick one.",
    ),
    ("Perfil de energia do notebook", "Laptop power profile"),
    (
        "Deixar a bateria em 80% quando o notebook fica sempre na tomada aumenta muito a vida útil dela.",
        "Keeping the battery at 80% when the laptop is always plugged in greatly extends its lifespan.",
    ),
    ("Parar de carregar em", "Stop charging at"),
    (" (agora)", " (now)"),
    (
        "Xwayland só na integrada",
        "Xwayland on the integrated GPU only",
    ),
    (
        "Instala /usr/local/bin/Xwayland que usa só o Mesa (pede a senha). Jogos X11 escolhidos para a NVIDIA continuam indo para ela. Vale no próximo login.",
        "Installs /usr/local/bin/Xwayland that uses only Mesa (asks for the password). X11 games set to the NVIDIA still go to it. Applies at the next login.",
    ),
    (
        "usar só a integrada no Xwayland",
        "use only the integrated GPU in Xwayland",
    ),
    (
        "O Xwayland pode prender a NVIDIA à toa.",
        "Xwayland may hold the NVIDIA for no reason.",
    ),
    (
        "Inicialização de volta ao normal (próximo login)",
        "Startup back to normal (next login)",
    ),
    ("nada novo", "nothing new"),
    (
        "Os programas da inicialização (ZapZap, notificadores...) e os serviços do usuário (ex.: Sunshine) \
                             passam a usar só a 680M, então não acordam a NVIDIA. Lançadores de jogos (Steam, Heroic, Lutris) \
                             ficam como estão: os jogos continuam na NVIDIA. Vale a partir do próximo login.",
        "Startup programs (ZapZap, notifiers...) and user services (e.g. Sunshine) \
                             use only the 680M, so they do not wake the NVIDIA. Game launchers (Steam, Heroic, Lutris) \
                             stay as they are: games keep running on the NVIDIA. Applies from the next login.",
    ),
    (
        "Tudo que abre com o sistema na integrada",
        "Everything that starts with the system on the integrated GPU",
    ),
    (
        "não mexer (dorme quando ninguém usa)",
        "leave as is (sleeps when nobody uses it)",
    ),
    (
        "Troca na hora, sem sair da sessão: \"desligada\" tira a placa do sistema (zero consumo, nada a acorda, o KDE segue \
                         só na integrada e o HDMI não funciona); \"disponível\" devolve e o KDE a adiciona sozinho. \
                         Só desliga se nenhum programa estiver usando a NVIDIA.",
        "Switches right away, without leaving the session: \"off\" removes the card from the system (zero power, nothing wakes it, KDE carries on \
                         with the integrated GPU only and HDMI does not work); \"available\" brings it back and KDE adds it by itself. \
                         It only turns off if no program is using the NVIDIA.",
    ),
    ("nenhuma tela", "no display"),
    ("parada no limite", "holding at the limit"),
    (
        "Sem turbo, governor econômico, EPB economia, potência limitada a 60% e LAVD em economia.",
        "No turbo, power-saving governor, EPB power saving, power limited to 60% and LAVD in power-saving mode.",
    ),
    (
        "Os dois sockets em desempenho, uncore no máximo, THP sempre e sem balanceamento NUMA automático.",
        "Both sockets in performance, uncore at maximum, THP always and no automatic NUMA balancing.",
    ),
    (
        "Socket da placa de vídeo em desempenho máximo, uncore no máximo, turbo, sem balanceamento NUMA automático e escalonador LAVD em modo jogo.",
        "Graphics card's socket at maximum performance, uncore at maximum, turbo, no automatic NUMA balancing and the LAVD scheduler in gaming mode.",
    ),
    (
        "Tudo como o sistema estava.",
        "Everything as the system was.",
    ),
    (
        "Um clique ajusta processadores, memória e escalonador juntos. \"Normal\" devolve tudo como o sistema estava.",
        "One click tunes processors, memory and scheduler together. \"Normal\" puts everything back as the system was.",
    ),
    ("equilibrado (economia)", "balanced (power saving)"),
    ("equilibrado (desempenho)", "balanced (performance)"),
    ("Voltar ao automático", "Back to automatic"),
    ("IA / ROCm", "AI / ROCm"),
    ("jogo / gráfico", "game / graphics"),
    ("Uso da GPU", "GPU usage"),
    ("Programa", "Program"),
    (
        "Nenhuma escolha salva: tudo no automático.",
        "No saved choices: everything on automatic.",
    ),
    ("Importar escolhas", "Import choices"),
    (
        "Traz as escolhas do arquivo. Núcleos e placas de outro PC são adaptados (voltam para o automático).",
        "Brings in the choices from the file. Cores and cards from another PC are adapted (they go back to automatic).",
    ),
    ("Importar", "Import"),
    (
        "arquivo .json exportado (caminho completo)",
        "exported .json file (full path)",
    ),
    (
        "Grava todas as escolhas num arquivo, para levar a outro PC ou compartilhar.",
        "Writes all choices to a file, to take to another PC or share.",
    ),
    ("Exportar…", "Export…"),
    (
        "Programas com escolhas próprias. Os que não estão aqui ficam no automático.",
        "Programs with their own choices. The ones not listed here stay on automatic.",
    ),
    (
        "Atalho do menu voltou ao original",
        "Menu shortcut restored to the original",
    ),
    (
        "Copiado! Cole em Steam → Propriedades do jogo → Opções de inicialização.",
        "Copied! Paste into Steam → game Properties → Launch options.",
    ),
    (
        "Faz o atalho do menu abrir este programa pelo numapilot, para a GPU escolhida valer sempre.",
        "Makes the menu shortcut open this program through numapilot, so the chosen GPU always applies.",
    ),
    (
        "O atalho do menu já abre pelo numapilot. Clique para desfazer.",
        "The menu shortcut already opens through numapilot. Click to undo.",
    ),
    ("Usar no menu", "Use in the menu"),
    ("No menu: sim", "In the menu: yes"),
    (
        "Faz este programa abrir quando você entra na sessão (aba Inicialização tem tudo)",
        "Makes this program open when you log in (the Startup tab has everything)",
    ),
    (
        "Já abre quando você entra na sessão. Clique para não abrir mais (aba Inicialização tem tudo).",
        "Already opens when you log in. Click to stop that (the Startup tab has everything).",
    ),
    ("Abrir ao iniciar", "Open at startup"),
    ("Abre ao iniciar: sim", "Opens at startup: yes"),
    (
        "Fecha e abre de novo com a CPU e a GPU escolhidas",
        "Closes and opens it again with the chosen CPU and GPU",
    ),
    ("Reabrir", "Reopen"),
    ("Copiar opção Steam", "Copy Steam option"),
    (
        "Núcleos, memória, prioridade, GPU e perfis prontos",
        "Cores, memory, priority, GPU and ready-made profiles",
    ),
    (
        "reabra o programa para trocar a GPU",
        "reopen the program to switch the GPU",
    ),
    (
        "Mesma GPU do jogo (lsfg-vk oficial)",
        "Same GPU as the game (official lsfg-vk)",
    ),
    ("Lossless Scaling: mesma GPU", "Lossless Scaling: same GPU"),
    (
        "Parte da memória está no outro socket: cada acesso a ela é mais lento.",
        "Part of the memory is on the other socket: every access to it is slower.",
    ),
    ("sem GPU", "no GPU"),
    ("os dois sockets", "both sockets"),
    ("livre (todos)", "free (all)"),
    (
        "Memória em uso pelo programa (somando os processos dele)",
        "Memory in use by the program (adding up its processes)",
    ),
    (
        "Memória em uso / limite escolhido em Ajustar…",
        "Memory in use / limit chosen in Adjust…",
    ),
    (
        "pode fechar esta janela; o comando continua em segundo plano",
        "you can close this window; the command keeps running in the background",
    ),
    ("com erro", "failed"),
    ("concluído", "finished"),
    ("rodando…", "running…"),
    (
        "Espere o comando anterior terminar",
        "Wait for the previous command to finish",
    ),
    ("Nenhuma GPU", "No GPU"),
    (", monitor próprio", ", own monitor"),
    ("Os dois sockets", "Both sockets"),
    ("Socket da GPU escolhida", "Socket of the chosen GPU"),
    (
        "Gráficos: último minuto, na mesma escala (linha cheia = leitura/download).",
        "Charts: last minute, on the same scale (solid line = read/download).",
    ),
    ("Rede", "Network"),
    ("Medindo...", "Measuring..."),
    ("Discos e rede", "Disks and network"),
    ("Diagnóstico", "Diagnosis"),
    (
        "Diagnóstico completo (gfx906-rocm doctor)",
        "Full diagnosis (gfx906-rocm doctor)",
    ),
    ("CPUs \\ memória", "CPUs \\ memory"),
    (
        "Ainda não medido. Clique em \"Medir agora\".",
        "Not measured yet. Click \"Measure now\".",
    ),
    (
        "Medindo latência e banda de memória",
        "Measuring memory latency and bandwidth",
    ),
    (
        "Leva ~10 s; o PC fica ocupado durante o teste",
        "Takes ~10 s; the PC is busy during the test",
    ),
    ("Medir agora", "Measure now"),
    (
        "Latência e banda entre sockets (medidas)",
        "Latency and bandwidth between sockets (measured)",
    ),
    ("Ainda não medido.", "Not measured yet."),
    (
        "Pequena demais para usar como critério: a ordem do sistema continua.",
        "Too small to use as a criterion: the system order stays.",
    ),
    (
        "Em uso: os programas com número de núcleos escolhido ficam nos mais rápidos.",
        "In use: programs with a chosen number of cores get the fastest ones.",
    ),
    (
        "Medindo a latência do cache por núcleo",
        "Measuring cache latency per core",
    ),
    (
        "Mede o acesso ao cache L3 a partir de cada núcleo (alguns segundos, com o PC parado). Se a diferença for maior que o ruído, as escolhas de núcleos, inclusive o turbo máximo, passam a preferir os mais rápidos.",
        "Measures L3 cache access from each core (a few seconds, with the PC idle). If the difference is larger than the noise, core choices, including maximum turbo, start preferring the fastest ones.",
    ),
    ("Medir núcleos", "Measure cores"),
    ("Latência do cache por núcleo", "Cache latency per core"),
    (
        "O irqbalance está rodando e pode desfazer o ajuste das interrupções das GPUs: \
                     sudo systemctl disable --now irqbalance",
        "irqbalance is running and may undo the GPU interrupt tuning: \
                     sudo systemctl disable --now irqbalance",
    ),
    (
        "Interrupções das GPUs: atendidas pelo socket de cada placa.",
        "GPU interrupts: handled by each card's socket.",
    ),
    ("indisponível", "unavailable"),
    (
        "performance = frequência máxima sempre (jogos); schedutil/powersave = economiza energia.",
        "performance = always maximum frequency (games); schedutil/powersave = saves power.",
    ),
    ("Governador", "Governor"),
    (
        "Páginas de memória que precisaram ser criadas no outro socket (numastat other_node/numa_miss), por segundo.",
        "Memory pages that had to be created on the other socket (numastat other_node/numa_miss), per second.",
    ),
    (
        "Alocações no outro socket",
        "Allocations on the other socket",
    ),
    ("Memória em uso", "Memory in use"),
    (
        "Memória e frequência por socket",
        "Memory and frequency per socket",
    ),
    (
        "Cada quadrado é um núcleo físico (dividido nos 2 threads). Cor = uso; ponto branco = algum programa \
                     está preso a ele (passe o mouse para ver).",
        "Each square is a physical core (split into its 2 threads). Color = usage; white dot = some program \
                     is pinned to it (hover to see).",
    ),
    ("Mapa de núcleos", "Core map"),
    (
        "Passe o mouse num gráfico para ver o núcleo e a frequência; ponto amarelo = algum programa está preso a ele.",
        "Hover over a chart to see the core and the frequency; yellow dot = some program is pinned to it.",
    ),
    (
        "Temperatura do processador (último minuto, 30–100 °C)",
        "Processor temperature (last minute, 30–100 °C)",
    ),
    (
        "Os estados de economia profundos (C3/C6) estão desligados: todos os núcleos contam como \
                         acordados e o turbo fica no valor de todos os núcleos. Ligue-os em Máquina → Processador.",
        "The deep idle states (C3/C6) are off: every core counts as \
                         awake and turbo stays at the all-core value. Turn them on in Machine → Processor.",
    ),
    (
        "A tabela de turbo vem do serviço do numapilot (só processadores Intel).",
        "The turbo table comes from the numapilot service (Intel processors only).",
    ),
    (
        "Faz o atalho do menu abrir este programa pelo numapilot, para a escolha de placa de vídeo valer. Dá para desfazer em Escolhas salvas.",
        "Makes the menu shortcut open this program through numapilot, so the graphics card choice applies. Can be undone in Saved choices.",
    ),
    ("Ajustar o menu", "Adjust the menu"),
    (
        "Aberto pelo menu, este programa já passa pelo numapilot (a escolha de placa vale).",
        "Opened from the menu, this program already goes through numapilot (the card choice applies).",
    ),
    ("menu ajustado", "menu adjusted"),
    ("aberto agora", "open now"),
    ("escolha salva", "saved choice"),
    (
        "Nenhum programa encontrado com essa busca.",
        "No program found with this search.",
    ),
    (
        "A escolha fica salva e vale quando o programa abrir. Processador e memória valem em qualquer \
                     abertura; para a placa de vídeo, abra pelo menu ajustado (opção no próprio Ajustar…) ou pelo Reabrir.",
        "The choice is saved and applies when the program starts. Processor and memory apply on any \
                     start; for the graphics card, open it from the adjusted menu (option in Adjust… itself) or with Reopen.",
    ),
    (
        "🔍 Nome do programa, executável ou origem",
        "🔍 Program name, executable or source",
    ),
    ("Encerrando", "Ending"),
    ("Parada forçada:", "Forced stop:"),
    (
        "O que não foi salvo nele se perde.",
        "Anything unsaved in it is lost.",
    ),
    (
        "não podem ser encerrados por aqui",
        "cannot be ended from here",
    ),
    (
        "Nenhum programa sendo usado agora. Abra algo (um jogo, o Blender, o llama-server...) e ele aparece aqui.",
        "No program in use right now. Open something (a game, Blender, llama-server...) and it shows up here.",
    ),
    (
        "Clique numa linha para selecionar; na seta (ou dois cliques) para ver os processos e escolher processador e placa. CPU muda na hora; a GPU vale quando o programa abre.",
        "Click a row to select it; the arrow (or double-click) shows the processes and lets you choose processor and card. CPU changes right away; the GPU applies when the program starts.",
    ),
    (
        "Lista os programas instalados (menu, Flatpak, Snap, AppImage, jogos da Steam) para escolher processador, memória e placa sem precisar abrir o programa antes.",
        "Lists the installed programs (menu, Flatpak, Snap, AppImage, Steam games) to choose processor, memory and card without having to open the program first.",
    ),
    ("🔍 Buscar", "🔍 Search"),
    ("Steam: jogos", "Steam: games"),
    ("Ver lista", "View list"),
    (
        "Steam: remover de todos os jogos",
        "Steam: remove from all games",
    ),
    ("Remover de todos", "Remove from all"),
    (
        "Steam: aplicar em todos os jogos",
        "Steam: apply to all games",
    ),
    (
        "Coloca \"numapilot run -- %command%\" nas opções de inicialização de todos os jogos instalados (mantém o que já existia). Feche a Steam antes.",
        "Puts \"numapilot run -- %command%\" in the launch options of every installed game (keeps what was already there). Close Steam first.",
    ),
    ("Aplicar em todos os jogos", "Apply to all games"),
    (
        "Pede para o programa fechar (SIGTERM), como fechar a janela",
        "Asks the program to close (SIGTERM), like closing the window",
    ),
    ("■ Encerrar", "■ End"),
    (
        "Fecha na hora (SIGKILL). O que não foi salvo se perde.",
        "Closes it right away (SIGKILL). Anything unsaved is lost.",
    ),
    ("⊗ Forçar parada", "⊗ Force stop"),
    (
        "Selecione um programa na lista",
        "Select a program in the list",
    ),
    ("últimas 200 linhas", "last 200 lines"),
    ("Copiar", "Copy"),
    ("Reiniciar", "Restart"),
    ("No boot", "At boot"),
    ("Estado", "State"),
    ("Serviço", "Service"),
    ("Nenhum serviço nesse filtro.", "No service in this filter."),
    (
        "Para escolher o que abre no boot, use a aba Inicialização.",
        "To choose what starts at boot, use the Startup tab.",
    ),
    ("Todos", "All"),
    ("Com falha", "Failed"),
    ("Parados", "Stopped"),
    ("Rodando", "Running"),
    ("Do usuário", "User"),
    ("Do sistema", "System"),
    (
        "Do sistema: rodam desde o boot. Iniciar, parar e reiniciar pedem a senha.",
        "System: they run from boot. Start, stop and restart ask for the password.",
    ),
    (
        "Do seu usuário: rodam a partir do login, sem pedir senha.",
        "Your user's: they run from login, with no password.",
    ),
    (
        "UEFI, TPM, núcleos e memória do socket da placa; depois instale o Windows pelo virt-manager",
        "UEFI, TPM, cores and memory from the card's socket; then install Windows through virt-manager",
    ),
    ("Criar a VM", "Create the VM"),
    (" núcleos", " cores"),
    ("Memória / núcleos / disco", "Memory / cores / disk"),
    ("ISO do Windows", "Windows ISO"),
    ("Abrir virt-manager", "Open virt-manager"),
    ("Devolvendo a placa", "Giving the card back"),
    ("Devolver ao Linux", "Give back to Linux"),
    ("Preparando a VM", "Preparing the VM"),
    (
        "Prende a placa no vfio-pci no boot, liga o IOMMU e refaz a imagem de boot (pede a senha). Reinicie depois.",
        "Binds the card to vfio-pci at boot, enables the IOMMU and rebuilds the boot image (asks for the password). Reboot afterwards.",
    ),
    ("Preparar a placa", "Prepare the card"),
    ("Conferir", "Check"),
    ("Placa da VM:", "VM card:"),
    (
        "Uma placa de vídeo fica só para o Windows numa VM (VFIO). O Linux continua na outra. Alguns anti-cheats (ex.: Vanguard) recusam VM: confira o jogo antes.",
        "One graphics card is reserved for Windows in a VM (VFIO). Linux stays on the other. Some anti-cheats (e.g. Vanguard) refuse VMs: check the game first.",
    ),
    (
        "Máquina virtual Windows (jogos com anti-cheat)",
        "Windows virtual machine (games with anti-cheat)",
    ),
    ("Desempenho", "Performance"),
    ("Resultado", "Result"),
    ("Versão", "Version"),
    ("O quê", "What"),
    ("Data", "Date"),
    ("Histórico das atualizações", "Update history"),
    ("Novidades da comunidade", "Community news"),
    (
        "Validando as versões atuais",
        "Validating the current versions",
    ),
    (
        "Roda os testes nas GPUs e grava o desempenho de agora como referência",
        "Runs the tests on the GPUs and saves the current performance as the reference",
    ),
    ("Validar as versões atuais", "Validate the current versions"),
    ("Atualizando com validação", "Updating with validation"),
    (
        "Atualizar agora (com validação)",
        "Update now (with validation)",
    ),
    ("Procurando versões novas", "Looking for new versions"),
    ("Verificar agora", "Check now"),
    ("Ainda não verificado.", "Not checked yet."),
    (
        "Mantém a versão atual mesmo com atualização automática",
        "Keeps the current version even with automatic updates",
    ),
    ("Travar", "Pin"),
    ("Disponível", "Available"),
    ("Em uso", "In use"),
    ("ROCm nativo", "Native ROCm"),
    ("Imagem ROCm", "ROCm image"),
    ("Laboratório (PyTorch)", "Lab (PyTorch)"),
    (
        "Vulkan (GPUs sem ROCm: o laboratório usa a CPU e o servidor de IA usa a GPU pelo Vulkan)",
        "Vulkan (GPUs without ROCm: the lab uses the CPU and the AI server uses the GPU through Vulkan)",
    ),
    ("ROCm oficial da AMD", "AMD's official ROCm"),
    ("MI50 com ROCm da comunidade", "MI50 with community ROCm"),
    (
        "Preferência de atualização salva",
        "Update preference saved",
    ),
    ("só me avisar", "just notify me"),
    (
        "instalar se passar na validação",
        "install if it passes validation",
    ),
    ("Quando houver versão nova:", "When there is a new version:"),
    ("Sem rede local detectada.", "No local network detected."),
    (
        "O link tem a senha de acesso; não compartilhe. Fora de casa, use uma VPN (ex.: Tailscale).",
        "The link contains the access password; do not share it. Away from home, use a VPN (e.g. Tailscale).",
    ),
    ("Copiar link", "Copy link"),
    (
        "Aponte a câmera do celular ou abra:",
        "Point the phone camera or open:",
    ),
    ("Painel no celular", "Phone panel"),
    (
        "Ligar o painel (inicia junto com a sessão)",
        "Turn on the panel (starts with the session)",
    ),
    (
        "Veja temperaturas e uso, troque o perfil de um programa e ligue o laboratório/servidor de IA pelo celular (mesma rede).",
        "See temperatures and usage, switch a program's profile and turn on the lab/AI server from the phone (same network).",
    ),
    (
        "Baixa a versão nova, recompila e reinstala num terminal (pede a senha). Depois, reabra a interface.",
        "Downloads the new version, rebuilds and reinstalls in a terminal (asks for the password). Then reopen the interface.",
    ),
    ("Atualizar o numapilot", "Update numapilot"),
    ("origem desconhecida", "unknown source"),
    ("Adicionar", "Add"),
    ("nome (ex.: BC-250)", "name (e.g. BC-250)"),
    ("Backup dos PCs", "PC backup"),
    (
        "Guarda as configurações de cada PC ligado em ~/numapilot-backups (o painel no celular faz isso sozinho uma vez por dia)",
        "Stores the settings of each PC that is on in ~/numapilot-backups (the phone panel does this by itself once a day)",
    ),
    ("Backup de todos agora", "Back up all now"),
    (
        "Traz os perfis de programa (Ajustar…) que este PC ainda não tem",
        "Brings in the program profiles (Adjust…) this PC does not have yet",
    ),
    ("copiar perfis", "copy profiles"),
    (
        "Manda o pacote do Wake-on-LAN",
        "Sends the Wake-on-LAN packet",
    ),
    (
        "Cole o link do painel de outro PC (Configurações → Painel no celular, lá nele): o painel deste mostra todos juntos.",
        "Paste the link of another PC's panel (Settings → Phone panel, on that PC): this one's panel shows them all together.",
    ),
    ("Meus outros PCs", "My other PCs"),
    ("abrir no navegador", "open in the browser"),
    ("Aviso de teste enviado", "Test notification sent"),
    (
        "Se chegou, os avisos estão funcionando.",
        "If this arrived, notifications are working.",
    ),
    ("Teste do numapilot", "numapilot test"),
    ("Mandar teste", "Send test"),
    ("Salvar", "Save"),
    ("tarefas prontas", "finished tasks"),
    ("outros PCs", "other PCs"),
    ("discos/memória/bateria", "disks/memory/battery"),
    ("GPU quente", "Hot GPU"),
    ("Token (opcional)", "Token (optional)"),
    ("Tópico", "Topic"),
    ("Servidor", "Server"),
    ("Ligado", "On"),
    (
        "Pelo app ntfy (Android/iOS, grátis): GPU quente, disco falhando, servidor de IA caiu, PC saiu da rede, undervolt pronto. Instale o ntfy e assine o tópico (ou leia o QR code).",
        "Through the ntfy app (Android/iOS, free): hot GPU, failing disk, AI server down, PC off the network, undervolt done. Install ntfy and subscribe to the topic (or scan the QR code).",
    ),
    (
        "• Configurações avançadas: /etc/numapilot.toml (regras) e /etc/gfx906-rocm.conf (laboratório/ROCm).",
        "• Advanced settings: /etc/numapilot.toml (rules) and /etc/gfx906-rocm.conf (lab/ROCm).",
    ),
    (
        "• Lossless Scaling (lsfg-vk): o oficial gera os quadros na mesma GPU do jogo.",
        "• Lossless Scaling (lsfg-vk): the official one generates the frames on the same GPU as the game.",
    ),
    (
        "• Jogo numa MI50: escolha \"MI50 — jogo/gráfico\" na aba Programas.",
        "• Game on an MI50: choose \"MI50 — game/graphics\" in the Programs tab.",
    ),
    ("Dicas", "Tips"),
    (
        "computação; também roda jogos via PRIME",
        "compute; also runs games through PRIME",
    ),
    (
        "computação + monitor ligado nela",
        "compute + monitor connected to it",
    ),
    ("vídeo (monitor ligado)", "video (monitor connected)"),
    ("Relatório de diagnóstico", "Diagnosis report"),
    (
        "Salva ~/numapilot-relatorio.txt com o diagnóstico e as informações do sistema",
        "Saves ~/numapilot-relatorio.txt with the diagnosis and system information",
    ),
    ("Gerar relatório para enviar", "Generate report to send"),
    ("Backup das configurações", "Settings backup"),
    (
        "Salva ~/numapilot-backup-DATA.tar.gz com perfis, ajustes da máquina, ventoinhas, GPUs, laboratório e parâmetros do kernel. Guarde fora do disco do sistema; para restaurar: ./install.sh --restore ARQUIVO",
        "Saves ~/numapilot-backup-DATA.tar.gz with profiles, machine settings, fans, GPUs, lab and kernel parameters. Keep it off the system disk; to restore: ./install.sh --restore FILE",
    ),
    ("Fazer backup das configurações", "Back up the settings"),
    ("Verificar tudo", "Check everything"),
    (
        "Testa cada parte no seu hardware (NUMA, cgroups, seleção de GPU no Vulkan, ROCm, ventoinhas, OverDrive, ReBAR, Steam) e diz o que corrigir.",
        "Tests each part on your hardware (NUMA, cgroups, Vulkan GPU selection, ROCm, fans, OverDrive, ReBAR, Steam) and says what to fix.",
    ),
    (
        "Fica em /etc/modprobe.d/numapilot.conf e a imagem de boot é refeita: vale para qualquer kernel, inclusive depois de atualizar. Pede a senha.",
        "Lives in /etc/modprobe.d/numapilot.conf and the boot image is rebuilt: works for any kernel, even after updates. Asks for the password.",
    ),
    (
        "Definido na linha do kernel (amdgpu.ppfeaturemask): mude no bootloader ou remova de lá para controlar por aqui.",
        "Set on the kernel command line (amdgpu.ppfeaturemask): change it in the bootloader or remove it there to control it from here.",
    ),
    ("desliga ao reiniciar", "turns off after reboot"),
    ("liga ao reiniciar", "turns on after reboot"),
    (
        "OverDrive das placas AMD (undervolt/overclock)",
        "OverDrive of the AMD cards (undervolt/overclock)",
    ),
    ("Opções de boot", "Boot options"),
    ("Atualização semanal", "Weekly update"),
    (
        "Verificar toda semana (laboratório, llama.cpp e ROCm)",
        "Check every week (lab, llama.cpp and ROCm)",
    ),
    ("Atualizações", "Updates"),
    (
        "Desmarque o que você não usa neste PC para o menu ficar mais curto. Nada é desligado: é só o menu.",
        "Uncheck what you do not use on this PC to make the menu shorter. Nothing is turned off: it is only the menu.",
    ),
    (
        "Ícone na bandeja não abre mais sozinho",
        "Tray icon no longer starts by itself",
    ),
    ("Ícone na bandeja ativado", "Tray icon enabled"),
    (
        "Ícone na bandeja ao iniciar a sessão (atalhos para o laboratório, IA e esta janela)",
        "Tray icon when the session starts (shortcuts to the lab, AI and this window)",
    ),
    (
        "parado — sudo systemctl enable --now numapilot",
        "stopped — sudo systemctl enable --now numapilot",
    ),
    ("Serviço numapilot:", "numapilot service:"),
    (
        "Adicionado: abre no próximo login",
        "Added: opens at the next login",
    ),
    (
        "(útil para deixar a rede ou outro programa subir antes)",
        "(useful to let the network or another program come up first)",
    ),
    ("Esperar antes de abrir:", "Wait before opening:"),
    (
        "Para IA nas GPUs do servidor, use numapilot run: numapilot run --gpu compute -- llama-server -m modelo.gguf",
        "For AI on the server's GPUs, use numapilot run: numapilot run --gpu compute -- llama-server -m modelo.gguf",
    ),
    ("Comando", "Command"),
    (
        "Se não for executável: o script roda pelo sh; AppImage e binário ficam executáveis.",
        "If it is not executable: the script runs through sh; AppImage and binary are made executable.",
    ),
    ("Escolher…", "Choose…"),
    (
        "/caminho/do/programa.AppImage ou script.sh",
        "/path/to/program.AppImage or script.sh",
    ),
    (
        "Programas seus abertos agora (os mesmos da aba Programas):",
        "Your programs open now (the same as in the Programs tab):",
    ),
    (
        "🔍 nome do programa (Steam, ZapZap...)",
        "🔍 program name (Steam, ZapZap...)",
    ),
    ("Abrir no login: comando", "Open at login: command"),
    ("Abrir no login: arquivo", "Open at login: file"),
    (
        "Abrir no login: programa aberto agora",
        "Open at login: program open now",
    ),
    (
        "Abrir no login: programa instalado",
        "Open at login: installed program",
    ),
    ("Desinstalar", "Uninstall"),
    (
        "O programa está aberto: ele vai ser encerrado antes.",
        "The program is open: it will be closed first.",
    ),
    ("Comandos exatos", "Exact commands"),
    (
        "Apagar também as configurações e a pasta do programa",
        "Also delete the settings and the program's folder",
    ),
    (
        "Vai fazer, num terminal (o que pede senha pede lá):",
        "It will run, in a terminal (whatever needs the password asks there):",
    ),
    ("Últimas linhas do log", "Last log lines"),
    ("Desinstalar…", "Uninstall…"),
    ("Ver na aba Programas", "View in the Programs tab"),
    ("não está rodando", "not running"),
    ("Pasta do programa", "Program folder"),
    ("Tamanho", "Size"),
    ("Descrição", "Description"),
    ("Origem", "Source"),
    ("Abre por", "Started by"),
    ("Abrir pasta", "Open folder"),
    ("não abre mais", "no longer starts"),
    (
        "Remove o programa do jeito certo para a origem dele (pacote, Flatpak, npm, pipx, arquivo...)",
        "Removes the program the right way for its source (package, Flatpak, npm, pipx, file...)",
    ),
    (
        "Apaga este atalho de inicialização (não desinstala o programa)",
        "Deletes this startup shortcut (does not uninstall the program)",
    ),
    ("Remover", "Remove"),
    (
        "De onde vem: arquivo, programa, pacote, pastas",
        "Where it comes from: file, program, package, folders",
    ),
    ("Detalhes", "Details"),
    (
        "Ligar no próximo login/boot",
        "Turn on at the next login/boot",
    ),
    (
        "Desligar no próximo login/boot",
        "Turn off at the next login/boot",
    ),
    ("Qualquer linha de comando", "Any command line"),
    ("Comando…", "Command…"),
    ("Escolhe o arquivo", "Picks the file"),
    (
        "Arquivo (AppImage, script .sh, binário)…",
        "File (AppImage, .sh script, binary)…",
    ),
    (
        "Um dos programas da aba Programas",
        "One of the programs in the Programs tab",
    ),
    ("Programa aberto agora…", "Program open now…"),
    (
        "Procura nos programas do menu",
        "Searches the menu programs",
    ),
    ("Programa instalado…", "Installed program…"),
    ("+ Adicionar", "+ Add"),
    (
        "Desligar só vale para o próximo login ou boot: o que está aberto continua aberto.",
        "Turning off only applies to the next login or boot: what is open stays open.",
    ),
    (
        "Dica: escolha a GPU que tem o monitor do jogo, se ela codificar H.264/HEVC. Para jogos na MI50 \
             a imagem é capturada dela; para menos atraso, deixe o jogo e a codificação na mesma placa.",
        "Tip: choose the GPU that has the game's monitor, if it encodes H.264/HEVC. For games on the MI50 \
             the image is captured from it; for less delay, keep the game and the encoding on the same card.",
    ),
    (
        "Software: ultrafast gasta menos CPU (imagem pior); veryfast fica mais bonito mas pesa. Comece em superfast a 1080p60.",
        "Software: ultrafast uses less CPU (worse image); veryfast looks better but is heavier. Start with superfast at 1080p60.",
    ),
    (
        "Velocidade salva; reinicie o Sunshine",
        "Speed saved; restart Sunshine",
    ),
    ("padrão (superfast)", "default (superfast)"),
    (
        "Codificador salvo; reinicie o Sunshine",
        "Encoder saved; restart Sunshine",
    ),
    ("Codificador", "Encoder"),
    (
        "GPU de codificação salva; reinicie o Sunshine para valer",
        "Encoding GPU saved; restart Sunshine to apply",
    ),
    ("monitor ligado", "monitor connected"),
    ("o Sunshine escolhe", "Sunshine chooses"),
    ("GPU que codifica o vídeo", "GPU that encodes the video"),
    ("Painel do Sunshine", "Sunshine panel"),
    ("Ligando o Sunshine", "Turning Sunshine on"),
    ("Reiniciando o Sunshine", "Restarting Sunshine"),
    ("Desligando o Sunshine", "Turning Sunshine off"),
    (
        "ou pelo AUR: paru -S sunshine (ou o seu fork)",
        "or from the AUR: paru -S sunshine (or your fork)",
    ),
    ("Instalando o Sunshine", "Installing Sunshine"),
    ("Instalar (Flatpak)", "Install (Flatpak)"),
    (
        "O Sunshine não está instalado.",
        "Sunshine is not installed.",
    ),
    (
        "Jogue no celular, notebook ou TV pelo Moonlight. O jogo roda aqui e uma GPU (ou a CPU) \
             codifica o vídeo.",
        "Play on the phone, laptop or TV with Moonlight. The game runs here and a GPU (or the CPU) \
             encodes the video.",
    ),
    ("Streaming de jogos (Sunshine)", "Game streaming (Sunshine)"),
    ("Sair do ícone", "Quit the icon"),
    ("Abrir numapilot", "Open numapilot"),
    ("Não consegui abrir o numapilot", "Could not open numapilot"),
    ("nenhuma", "none"),
    ("economia", "power saving"),
    ("alta", "high"),
    ("baixa", "low"),
    ("limpar", "clear"),
    ("todos", "all"),
    ("melhor", "best"),
    ("limitar", "limit"),
    ("velocidade", "speed"),
    ("fixa", "fixed"),
    ("curva", "curve"),
    ("falta", "missing"),
    ("recomendado", "recommended"),
    ("deslocar", "offset"),
    ("travar", "lock"),
    ("personalizar", "customize"),
    ("filtrar:", "filter:"),
    ("rodando", "running"),
    ("copiar", "copy"),
    ("(escolha)", "(choose)"),
    ("recarregar", "reload"),
    ("abrir", "open"),
    ("desligado", "off"),
    ("ligado", "on"),
    ("adicionar", "add"),
    ("remover", "remove"),
    ("das", "from"),
    ("corrigir", "fix"),
    ("sim", "yes"),
    ("escolha", "choose"),
    ("ligar", "turn on"),
    ("liga", "turns on"),
    ("agendado", "scheduled"),
    ("velocidade:", "speed:"),
    ("desempenho", "performance"),
    ("borda", "edge"),
];
