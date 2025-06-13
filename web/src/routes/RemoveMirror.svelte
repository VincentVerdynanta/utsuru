<script lang="ts">
	let { mirror, states } = $props();

    let loading = $state(false);
    let loadingClass = $derived(loading ? "bg-red-300 cursor-not-allowed" : "bg-red-500 cursor-pointer");

    async function deleteMirror() {
        if (loading) return;
        loading = true;

        const response = await fetch("/api/mirrors?action=delete", {
            method: "POST",
            headers: {
                "Content-Type": "application/json"
            },
            body: JSON.stringify({
                id: mirror
            })
        });

        loading = false;
        if (response.status !== 200) return;
        await states.refreshItems();
        states.closeModal();
    }
</script>

<div class="px-4 py-2.5 flex flex-col text-sm text-gray-100">
    {#if states}
        <div class="flex">
            <span class="flex-auto uppercase tracking-widest">Delete Discord Live {mirror + 1}</span>
            <button class="my-0.5 cursor-pointer" onclick={() => states.closeModal()}>
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24">
                    <!-- Icon from Lucide by Lucide Contributors - https://github.com/lucide-icons/lucide/blob/main/LICENSE -->
                    <path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18 6L6 18M6 6l12 12"/>
                </svg>
            </button>
        </div>
        <div class="h-px mt-2 mb-3 bg-zinc-700"></div>
        <div class="flex flex-col space-y-4.5">
            <span class="mb-0">You are about to delete Discord Live {mirror + 1}.</span>
            <span>This action cannot be undone.</span>
            <span>Please confirm that you would like to proceed.</span>
            <div class="mb-3.5 flex flex-col items-center">
                <button class="py-1.5 px-2 flex items-center space-x-1 rounded-lg {loadingClass}" onclick={deleteMirror}>
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="-2 -2 28 28">
                        <!-- Icon from Lucide by Lucide Contributors - https://github.com/lucide-icons/lucide/blob/main/LICENSE -->
                        <path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 6h18m-2 0v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6m3 0V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2m-6 5v6m4-6v6"/>
                    </svg>
                    <span>Remove</span>
                </button>
            </div>
        </div>
    {/if}
</div>
