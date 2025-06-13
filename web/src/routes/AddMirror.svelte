<script lang="ts">
	let { states } = $props();

    let token = $state();
    let guild_id = $state();
    let channel_id = $state();

    let loading = $state(false);
    let loadingClass = $derived(loading ? "bg-indigo-300 cursor-not-allowed" : "bg-indigo-500 cursor-pointer");

    let responseText = $state([]);

    async function createMirror() {
        if (loading) return;
        loading = true;
        responseText = [];

        const response = await fetch("/api/mirrors?action=create", {
            method: "POST",
            headers: {
                "Content-Type": "application/json"
            },
            body: JSON.stringify({
                token,
                guild_id: JSON.rawJSON(guild_id),
                channel_id: JSON.rawJSON(channel_id)
            })
        });

        if (!response.body) return;

        const reader = response.body.getReader();
        const decoder = new TextDecoder();

        while (true) {
            const { value, done } = await reader.read();
            if (done) break;

            const chunk = decoder.decode(value, { stream: true });
            responseText.push(chunk);
        }

        const flush = decoder.decode();
        if (flush.length) responseText.push(flush);

        loading = false;
        if (responseText.at(-1) !== "success") return;
        await states.refreshItems();
        states.closeModal();
    }
</script>

<div class="px-4 py-2.5 flex flex-col text-sm text-gray-100">
    {#if states}
        <div class="flex">
            <span class="flex-auto uppercase tracking-widest">Create Discord Mirror</span>
            <button class="my-0.5 cursor-pointer" onclick={() => states.closeModal()}>
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24">
                    <!-- Icon from Lucide by Lucide Contributors - https://github.com/lucide-icons/lucide/blob/main/LICENSE -->
                    <path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18 6L6 18M6 6l12 12"/>
                </svg>
            </button>
        </div>
        <div class="h-px mt-2 mb-3 bg-zinc-700"></div>
        <div class="flex flex-col space-y-1">
            <input bind:value={token} type="text" class="min-w-64 h-8 px-3 bg-zinc-800 rounded-md border border-zinc-700" placeholder="Token">
            <input bind:value={guild_id} type="text" class="min-w-64 h-8 px-3 bg-zinc-800 rounded-md border border-zinc-700" placeholder="Guild ID">
            <input bind:value={channel_id} type="text" class="min-w-64 h-8 px-3 bg-zinc-800 rounded-md border border-zinc-700" placeholder="Voice Channel ID">
            <div class="mt-3 mb-1.5 flex flex-col items-center">
                <button class="py-1.5 px-2 mb-1.5 flex items-center space-x-1 rounded-lg {loadingClass}" onclick={createMirror}>
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24">
                        <!-- Icon from Lucide by Lucide Contributors - https://github.com/lucide-icons/lucide/blob/main/LICENSE -->
                        <path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14m-7-7v14"/>
                    </svg>
                    <span>Add</span>
                </button>
                {#if responseText.length}
                    <div class="flex items-center space-x-1.5">
                        {#if loading}
                            <svg class="animate-spin" xmlns="http://www.w3.org/2000/svg" fill="none" width="16" height="16" viewBox="0 0 24 24">
                                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                            </svg>
                        {/if}
                        <span>{responseText.at(-1)}</span>
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
